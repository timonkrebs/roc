use crate::docs::DocEntry::DetatchedDoc;
use crate::docs::TypeAnnotation::{Apply, BoundVariable, Record, TagUnion};
use inlinable_string::InlinableString;
use roc_can::scope::Scope;
use roc_collections::all::MutMap;
use roc_module::ident::ModuleName;
use roc_module::symbol::{IdentIds, Interns, ModuleId};
use roc_parse::ast;
use roc_parse::ast::CommentOrNewline;
use roc_parse::ast::{AssignedField, Def};
use roc_region::all::Located;

// Documentation generation requirements

#[derive(Debug)]
pub struct Documentation {
    pub name: String,
    pub version: String,
    pub docs: String,
    pub modules: Vec<(MutMap<ModuleId, ModuleDocumentation>, Interns)>,
}

#[derive(Debug)]
pub struct ModuleDocumentation {
    pub name: String,
    pub entries: Vec<DocEntry>,
    pub scope: Scope,
}

#[derive(Debug, Clone)]
pub enum DocEntry {
    DocDef(DocDef),
    DetatchedDoc(String),
}

#[derive(Debug, Clone)]
pub struct DocDef {
    pub name: String,
    pub type_vars: Vec<String>,
    pub type_annotation: Option<TypeAnnotation>,
    pub docs: Option<String>,
}

#[derive(Debug, Clone)]
pub enum TypeAnnotation {
    TagUnion {
        tags: Vec<Tag>,
        extension: Option<Box<TypeAnnotation>>,
    },
    BoundVariable(String),
    Apply {
        name: String,
        parts: Vec<TypeAnnotation>,
    },
    Record {
        fields: Vec<RecordField>,
    },
}
#[derive(Debug, Clone)]
pub enum RecordField {
    RecordField {
        name: String,
        type_annotation: TypeAnnotation,
    },
    OptionalField {
        name: String,
        type_annotation: TypeAnnotation,
    },
    LabelOnly {
        name: String,
    },
}

#[derive(Debug, Clone)]
pub struct Tag {
    pub name: String,
    pub values: Vec<TypeAnnotation>,
}

pub fn generate_module_docs<'a>(
    scope: Scope,
    module_name: ModuleName,
    ident_ids: &'a IdentIds,
    parsed_defs: &'a [Located<ast::Def<'a>>],
) -> ModuleDocumentation {
    let (entries, _) =
        parsed_defs
            .iter()
            .fold((vec![], None), |(acc, maybe_comments_after), def| {
                generate_entry_doc(ident_ids, acc, maybe_comments_after, &def.value)
            });

    ModuleDocumentation {
        name: module_name.as_str().to_string(),
        scope,
        entries,
    }
}

fn detatched_docs_from_comments_and_new_lines<'a>(
    comments_or_new_lines: &'a [roc_parse::ast::CommentOrNewline<'a>],
) -> Vec<String> {
    let mut detatched_docs: Vec<String> = Vec::new();

    let mut docs = String::new();

    for comment_or_new_line in comments_or_new_lines.iter() {
        match comment_or_new_line {
            CommentOrNewline::DocComment(doc_str) => {
                docs.push_str(doc_str);
                docs.push('\n');
            }

            CommentOrNewline::LineComment(_) | CommentOrNewline::Newline => {
                detatched_docs.push(docs.clone());
                docs = String::new();
            }
        }
    }

    detatched_docs
}

fn generate_entry_doc<'a>(
    ident_ids: &'a IdentIds,
    mut acc: Vec<DocEntry>,
    before_comments_or_new_lines: Option<&'a [roc_parse::ast::CommentOrNewline<'a>]>,
    def: &'a ast::Def<'a>,
) -> (
    Vec<DocEntry>,
    Option<&'a [roc_parse::ast::CommentOrNewline<'a>]>,
) {
    use roc_parse::ast::Pattern;

    match def {
        Def::SpaceBefore(sub_def, comments_or_new_lines) => {
            // Comments before a definition are attached to the current defition

            for detatched_doc in detatched_docs_from_comments_and_new_lines(comments_or_new_lines) {
                acc.push(DetatchedDoc(detatched_doc));
            }

            generate_entry_doc(ident_ids, acc, Some(comments_or_new_lines), sub_def)
        }

        Def::SpaceAfter(sub_def, comments_or_new_lines) => {
            let (new_acc, _) =
                // If there are comments before, attach to this definition
                generate_entry_doc(ident_ids, acc, before_comments_or_new_lines, sub_def);

            // Comments after a definition are attached to the next definition
            (new_acc, Some(comments_or_new_lines))
        }

        Def::Annotation(loc_pattern, _loc_ann) => match loc_pattern.value {
            Pattern::Identifier(identifier) => {
                // Check if the definition is exposed
                if ident_ids
                    .get_id(&InlinableString::from(identifier))
                    .is_some()
                {
                    let doc_def = DocDef {
                        name: identifier.to_string(),
                        type_annotation: None,
                        type_vars: Vec::new(),
                        docs: before_comments_or_new_lines.and_then(comments_or_new_lines_to_docs),
                    };
                    acc.push(DocEntry::DocDef(doc_def));
                }
                (acc, None)
            }

            _ => (acc, None),
        },
        Def::AnnotatedBody { ann_pattern, .. } => match ann_pattern.value {
            Pattern::Identifier(identifier) => {
                // Check if the definition is exposed
                if ident_ids
                    .get_id(&InlinableString::from(identifier))
                    .is_some()
                {
                    let doc_def = DocDef {
                        name: identifier.to_string(),
                        type_annotation: None,
                        type_vars: Vec::new(),
                        docs: before_comments_or_new_lines.and_then(comments_or_new_lines_to_docs),
                    };
                    acc.push(DocEntry::DocDef(doc_def));
                }
                (acc, None)
            }

            _ => (acc, None),
        },

        Def::Alias { name, vars, ann } => {
            let mut type_vars = Vec::new();

            for var in vars.iter() {
                if let Pattern::Identifier(ident_name) = var.value {
                    type_vars.push(ident_name.to_string());
                }
            }

            let doc_def = DocDef {
                name: name.value.to_string(),
                type_annotation: type_to_docs(ann.value),
                type_vars,
                docs: before_comments_or_new_lines.and_then(comments_or_new_lines_to_docs),
            };
            acc.push(DocEntry::DocDef(doc_def));

            (acc, None)
        }

        Def::Body(_, _) => (acc, None),

        Def::Expect(c) => todo!("documentation for tests {:?}", c),

        Def::NotYetImplemented(s) => todo!("{}", s),
    }
}

fn type_to_docs(type_annotation: ast::TypeAnnotation) -> Option<TypeAnnotation> {
    match type_annotation {
        ast::TypeAnnotation::TagUnion {
            tags,
            ext,
            final_comments: _,
        } => {
            let mut tags_to_render: Vec<Tag> = Vec::new();

            let mut any_tags_are_private = false;

            for tag in tags {
                match tag_to_doc(tag.value) {
                    None => {
                        any_tags_are_private = true;
                        break;
                    }
                    Some(tag_ann) => {
                        tags_to_render.push(tag_ann);
                    }
                }
            }

            if any_tags_are_private {
                None
            } else {
                let extension = match ext {
                    None => None,
                    Some(ext_type_ann) => type_to_docs(ext_type_ann.value).map(Box::new),
                };

                Some(TagUnion {
                    tags: tags_to_render,
                    extension,
                })
            }
        }
        ast::TypeAnnotation::BoundVariable(var_name) => Some(BoundVariable(var_name.to_string())),
        ast::TypeAnnotation::Apply(module_name, type_name, type_ann_parts) => {
            let mut name = String::new();

            if !module_name.is_empty() {
                name.push_str(module_name);
                name.push('.');
            }

            name.push_str(type_name);

            let mut parts: Vec<TypeAnnotation> = Vec::new();

            for type_ann_part in type_ann_parts {
                if let Some(part) = type_to_docs(type_ann_part.value) {
                    parts.push(part);
                }
            }

            Some(Apply { name, parts })
        }
        ast::TypeAnnotation::Record {
            fields,
            ext: _,
            final_comments: _,
        } => {
            let mut doc_fields = Vec::new();

            let mut any_fields_include_private_tags = false;

            for field in fields {
                match record_field_to_doc(field.value) {
                    None => {
                        any_fields_include_private_tags = true;
                        break;
                    }
                    Some(doc_field) => {
                        doc_fields.push(doc_field);
                    }
                }
            }
            if any_fields_include_private_tags {
                None
            } else {
                Some(Record { fields: doc_fields })
            }
        }
        ast::TypeAnnotation::SpaceBefore(&sub_type_ann, _) => type_to_docs(sub_type_ann),
        ast::TypeAnnotation::SpaceAfter(&sub_type_ann, _) => type_to_docs(sub_type_ann),
        _ => {
            // TODO "Implement type to docs")

            None
        }
    }
}

fn record_field_to_doc(field: ast::AssignedField<'_, ast::TypeAnnotation>) -> Option<RecordField> {
    match field {
        AssignedField::RequiredValue(name, _, type_ann) => {
            type_to_docs(type_ann.value).map(|type_ann_docs| RecordField::RecordField {
                name: name.value.to_string(),
                type_annotation: type_ann_docs,
            })
        }
        AssignedField::SpaceBefore(&sub_field, _) => record_field_to_doc(sub_field),
        AssignedField::SpaceAfter(&sub_field, _) => record_field_to_doc(sub_field),
        AssignedField::OptionalValue(name, _, type_ann) => {
            type_to_docs(type_ann.value).map(|type_ann_docs| RecordField::OptionalField {
                name: name.value.to_string(),
                type_annotation: type_ann_docs,
            })
        }
        AssignedField::LabelOnly(label) => Some(RecordField::LabelOnly {
            name: label.value.to_string(),
        }),
        AssignedField::Malformed(_) => None,
    }
}

// The Option here represents if it is private. Private tags
// evaluate to `None`.
fn tag_to_doc(tag: ast::Tag) -> Option<Tag> {
    match tag {
        ast::Tag::Global { name, args } => Some(Tag {
            name: name.value.to_string(),
            values: {
                let mut type_vars = Vec::new();

                for arg in args {
                    if let Some(type_var) = type_to_docs(arg.value) {
                        type_vars.push(type_var);
                    }
                }

                type_vars
            },
        }),
        ast::Tag::Private { .. } => None,
        ast::Tag::SpaceBefore(&sub_tag, _) => tag_to_doc(sub_tag),
        ast::Tag::SpaceAfter(&sub_tag, _) => tag_to_doc(sub_tag),
        ast::Tag::Malformed(_) => None,
    }
}

fn comments_or_new_lines_to_docs<'a>(
    comments_or_new_lines: &'a [roc_parse::ast::CommentOrNewline<'a>],
) -> Option<String> {
    let mut docs = String::new();

    for comment_or_new_line in comments_or_new_lines.iter() {
        match comment_or_new_line {
            CommentOrNewline::DocComment(doc_str) => {
                docs.push_str(doc_str);
                docs.push('\n');
            }
            CommentOrNewline::Newline | CommentOrNewline::LineComment(_) => {
                docs = String::new();
            }
        }
    }

    if docs.is_empty() {
        None
    } else {
        Some(docs)
    }
}
