use bumpalo::{collections::Vec, Bump};
use roc_parse::ast::{CommentOrNewline, Pattern, Spaces, TypeAnnotation};

use crate::{
    annotation::{Formattable, Newlines, Parens},
    collection::Braces,
    spaces::{fmt_spaces, INDENT},
    Buf,
};

#[derive(Copy, Clone, Debug)]
pub struct Sp<'a> {
    default_space: bool, // if true and comments is empty, use a space (' ')
    comments: &'a [CommentOrNewline<'a>],
}

impl<'a> Sp<'a> {
    pub fn empty() -> Sp<'a> {
        Sp {
            default_space: false,
            comments: &[],
        }
    }
    pub fn space() -> Sp<'a> {
        Sp {
            default_space: true,
            comments: &[],
        }
    }

    pub fn with_space(sp: &'a [CommentOrNewline<'a>]) -> Self {
        Sp {
            default_space: true,
            comments: sp,
        }
    }
}

impl<'a> From<&'a [CommentOrNewline<'a>]> for Sp<'a> {
    fn from(comments: &'a [CommentOrNewline<'a>]) -> Self {
        Sp {
            default_space: false,
            comments,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum Node<'a> {
    Literal(&'a str),
    Sequence {
        first: &'a Node<'a>,
        extra_indent_for_rest: bool,
        rest: &'a [(Sp<'a>, Node<'a>)],
    },
    DelimitedSequence(Braces, &'a [(Sp<'a>, Node<'a>)], Sp<'a>),

    // Temporary! TODO: translate these into proper Node elements
    TypeAnnotation(TypeAnnotation<'a>),
    Pattern(Pattern<'a>),
}

pub fn parens_around_node<'a, 'b: 'a>(
    arena: &'a Bump,
    item: Spaces<'b, Node<'b>>,
    allow_spaces_before: bool,
) -> Spaces<'a, Node<'a>> {
    Spaces {
        before: if allow_spaces_before {
            item.before
        } else {
            &[]
        },
        item: Node::DelimitedSequence(
            Braces::Round,
            arena.alloc_slice_copy(&[(
                if allow_spaces_before {
                    Sp::empty()
                } else {
                    item.before.into()
                },
                item.item,
            )]),
            Sp::empty(),
        ),
        // We move the comments/newlines to the outer scope, since they tend to migrate there when re-parsed
        after: item.after,
    }
}

pub trait Nodify<'a> {
    fn to_node<'b>(&'a self, arena: &'b Bump, parens: Parens) -> Spaces<'b, Node<'b>>
    where
        'a: 'b;
}

fn fmt_sp(buf: &mut Buf, sp: Sp<'_>, indent: u16) {
    if !sp.comments.is_empty() {
        fmt_spaces(buf, sp.comments.iter(), indent);
    } else if sp.default_space {
        buf.spaces(1);
    }
}

impl<'a> Formattable for Node<'a> {
    fn is_multiline(&self) -> bool {
        match self {
            Node::DelimitedSequence(_braces, lefts, right) => {
                right.comments.is_empty()
                    && lefts
                        .iter()
                        .any(|(sp, l)| l.is_multiline() || !sp.comments.is_empty())
            }
            Node::Sequence {
                first,
                extra_indent_for_rest: _,
                rest,
            } => {
                first.is_multiline()
                    || rest
                        .iter()
                        .any(|(sp, l)| l.is_multiline() || !sp.comments.is_empty())
            }
            Node::Literal(_) => false,
            Node::TypeAnnotation(type_annotation) => type_annotation.is_multiline(),
            Node::Pattern(pat) => pat.is_multiline(),
        }
    }

    fn format_with_options(&self, buf: &mut Buf, parens: Parens, newlines: Newlines, indent: u16) {
        match self {
            Node::DelimitedSequence(braces, lefts, right) => {
                buf.indent(indent);
                buf.push(braces.start());

                for (sp, l) in *lefts {
                    fmt_sp(buf, *sp, indent);
                    l.format_with_options(buf, parens, newlines, indent);
                }
                fmt_sp(buf, *right, indent);

                buf.indent(indent);
                buf.push(braces.end());
            }
            Node::Sequence {
                first,
                extra_indent_for_rest,
                rest,
            } => {
                buf.indent(indent);
                let cur_indent = buf.cur_line_indent();
                first.format_with_options(buf, parens, newlines, indent);
                let next_indent = if *extra_indent_for_rest {
                    cur_indent + INDENT
                } else {
                    indent
                };

                for (sp, l) in *rest {
                    fmt_sp(buf, *sp, next_indent);
                    l.format_with_options(buf, parens, newlines, next_indent);
                }
            }
            Node::Literal(text) => {
                buf.indent(indent);
                buf.push_str(text);
            }
            Node::TypeAnnotation(type_annotation) => {
                type_annotation.format_with_options(buf, parens, newlines, indent);
            }
            Node::Pattern(pat) => {
                pat.format_with_options(buf, parens, newlines, indent);
            }
        }
    }
}

pub struct NodeSequenceBuilder<'a> {
    first: Node<'a>,
    extra_indent_for_rest: bool,
    rest: Vec<'a, (Sp<'a>, Node<'a>)>,
}

impl<'a> NodeSequenceBuilder<'a> {
    pub fn new(
        arena: &'a Bump,
        first: Node<'a>,
        capacity: usize,
        extra_indent_for_rest: bool,
    ) -> Self {
        Self {
            first,
            extra_indent_for_rest,
            rest: Vec::with_capacity_in(capacity, arena),
        }
    }

    pub fn push(&mut self, sp: Sp<'a>, literal: Node<'a>) {
        self.rest.push((sp, literal));
    }

    pub fn build(self) -> Node<'a> {
        Node::Sequence {
            first: self.rest.bump().alloc(self.first),
            extra_indent_for_rest: self.extra_indent_for_rest,
            rest: self.rest.into_bump_slice(),
        }
    }
}
