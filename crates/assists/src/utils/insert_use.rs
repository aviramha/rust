use std::iter::{self, successors};

use algo::skip_trivia_token;
use ast::{edit::AstNodeEdit, PathSegmentKind, VisibilityOwner};
use either::Either;
use syntax::{
    algo,
    ast::{self, make, AstNode},
    Direction, InsertPosition, SyntaxElement, SyntaxNode, T,
};

use crate::assist_context::AssistContext;

/// Determines the containing syntax node in which to insert a `use` statement affecting `position`.
pub(crate) fn find_insert_use_container(
    position: &SyntaxNode,
    ctx: &AssistContext,
) -> Option<Either<ast::ItemList, ast::SourceFile>> {
    ctx.sema.ancestors_with_macros(position.clone()).find_map(|n| {
        if let Some(module) = ast::Module::cast(n.clone()) {
            return module.item_list().map(Either::Left);
        }
        Some(Either::Right(ast::SourceFile::cast(n)?))
    })
}

pub(crate) fn insert_use_statement(
    // Ideally the position of the cursor, used to
    position: &SyntaxNode,
    path_to_import: &str,
    ctx: &crate::assist_context::AssistContext,
    builder: &mut text_edit::TextEditBuilder,
) {
    insert_use(position.clone(), make::path_from_text(path_to_import), Some(MergeBehaviour::Full));
}

pub fn insert_use(
    where_: SyntaxNode,
    path: ast::Path,
    merge_behaviour: Option<MergeBehaviour>,
) -> SyntaxNode {
    let use_item = make::use_(make::use_tree(path.clone(), None, None, false));
    // merge into existing imports if possible
    if let Some(mb) = merge_behaviour {
        for existing_use in where_.children().filter_map(ast::Use::cast) {
            if let Some(merged) = try_merge_imports(&existing_use, &use_item, mb) {
                let to_delete: SyntaxElement = existing_use.syntax().clone().into();
                let to_delete = to_delete.clone()..=to_delete;
                let to_insert = iter::once(merged.syntax().clone().into());
                return algo::replace_children(&where_, to_delete, to_insert);
            }
        }
    }

    // either we weren't allowed to merge or there is no import that fits the merge conditions
    // so look for the place we have to insert to
    let (insert_position, add_blank) = find_insert_position(&where_, path);

    let to_insert: Vec<SyntaxElement> = {
        let mut buf = Vec::new();

        if add_blank == AddBlankLine::Before {
            buf.push(make::tokens::single_newline().into());
        }

        buf.push(use_item.syntax().clone().into());

        if add_blank == AddBlankLine::After {
            buf.push(make::tokens::single_newline().into());
        } else if add_blank == AddBlankLine::AfterTwice {
            buf.push(make::tokens::single_newline().into());
            buf.push(make::tokens::single_newline().into());
        }

        buf
    };

    algo::insert_children(&where_, insert_position, to_insert)
}

fn try_merge_imports(
    old: &ast::Use,
    new: &ast::Use,
    merge_behaviour: MergeBehaviour,
) -> Option<ast::Use> {
    // dont merge into re-exports
    if old.visibility().map(|vis| vis.pub_token()).is_some() {
        return None;
    }
    let old_tree = old.use_tree()?;
    let new_tree = new.use_tree()?;
    let merged = try_merge_trees(&old_tree, &new_tree, merge_behaviour)?;
    Some(old.with_use_tree(merged))
}

/// Simple function that checks if a UseTreeList is deeper than one level
fn use_tree_list_is_nested(tl: &ast::UseTreeList) -> bool {
    tl.use_trees().any(|use_tree| {
        use_tree.use_tree_list().is_some() || use_tree.path().and_then(|p| p.qualifier()).is_some()
    })
}

pub fn try_merge_trees(
    old: &ast::UseTree,
    new: &ast::UseTree,
    merge_behaviour: MergeBehaviour,
) -> Option<ast::UseTree> {
    let lhs_path = old.path()?;
    let rhs_path = new.path()?;

    let (lhs_prefix, rhs_prefix) = common_prefix(&lhs_path, &rhs_path)?;
    let lhs = old.split_prefix(&lhs_prefix);
    let rhs = new.split_prefix(&rhs_prefix);
    let lhs_tl = lhs.use_tree_list()?;
    let rhs_tl = rhs.use_tree_list()?;

    // if we are only allowed to merge the last level check if the paths are only one level deep
    // FIXME: This shouldn't work yet i think
    if merge_behaviour == MergeBehaviour::Last && use_tree_list_is_nested(&lhs_tl)
        || use_tree_list_is_nested(&rhs_tl)
    {
        return None;
    }

    let should_insert_comma = lhs_tl
        .r_curly_token()
        .and_then(|it| skip_trivia_token(it.prev_token()?, Direction::Prev))
        .map(|it| it.kind() != T![,])
        .unwrap_or(true);
    let mut to_insert: Vec<SyntaxElement> = Vec::new();
    if should_insert_comma {
        to_insert.push(make::token(T![,]).into());
        to_insert.push(make::tokens::single_space().into());
    }
    to_insert.extend(
        rhs_tl
            .syntax()
            .children_with_tokens()
            .filter(|it| it.kind() != T!['{'] && it.kind() != T!['}']),
    );
    let pos = InsertPosition::Before(lhs_tl.r_curly_token()?.into());
    let use_tree_list = lhs_tl.insert_children(pos, to_insert);
    Some(lhs.with_use_tree_list(use_tree_list))
}

/// Traverses both paths until they differ, returning the common prefix of both.
fn common_prefix(lhs: &ast::Path, rhs: &ast::Path) -> Option<(ast::Path, ast::Path)> {
    let mut res = None;
    let mut lhs_curr = first_path(&lhs);
    let mut rhs_curr = first_path(&rhs);
    loop {
        match (lhs_curr.segment(), rhs_curr.segment()) {
            (Some(lhs), Some(rhs)) if lhs.syntax().text() == rhs.syntax().text() => (),
            _ => break,
        }
        res = Some((lhs_curr.clone(), rhs_curr.clone()));

        match lhs_curr.parent_path().zip(rhs_curr.parent_path()) {
            Some((lhs, rhs)) => {
                lhs_curr = lhs;
                rhs_curr = rhs;
            }
            _ => break,
        }
    }

    res
}

/// What type of merges are allowed.
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum MergeBehaviour {
    /// Merge everything together creating deeply nested imports.
    Full,
    /// Only merge the last import level, doesn't allow import nesting.
    Last,
}

#[derive(Eq, PartialEq, PartialOrd, Ord)]
enum ImportGroup {
    // the order here defines the order of new group inserts
    Std,
    ExternCrate,
    ThisCrate,
    ThisModule,
    SuperModule,
}

impl ImportGroup {
    fn new(path: &ast::Path) -> ImportGroup {
        let default = ImportGroup::ExternCrate;

        let first_segment = match first_segment(path) {
            Some(it) => it,
            None => return default,
        };

        let kind = first_segment.kind().unwrap_or(PathSegmentKind::SelfKw);
        match kind {
            PathSegmentKind::SelfKw => ImportGroup::ThisModule,
            PathSegmentKind::SuperKw => ImportGroup::SuperModule,
            PathSegmentKind::CrateKw => ImportGroup::ThisCrate,
            PathSegmentKind::Name(name) => match name.text().as_str() {
                "std" => ImportGroup::Std,
                "core" => ImportGroup::Std,
                // FIXME: can be ThisModule as well
                _ => ImportGroup::ExternCrate,
            },
            PathSegmentKind::Type { .. } => unreachable!(),
        }
    }
}

fn first_segment(path: &ast::Path) -> Option<ast::PathSegment> {
    first_path(path).segment()
}

fn first_path(path: &ast::Path) -> ast::Path {
    successors(Some(path.clone()), ast::Path::qualifier).last().unwrap()
}

fn segment_iter(path: &ast::Path) -> impl Iterator<Item = ast::PathSegment> + Clone {
    path.syntax().children().flat_map(ast::PathSegment::cast)
}

#[derive(PartialEq, Eq)]
enum AddBlankLine {
    Before,
    After,
    AfterTwice,
}

fn find_insert_position(
    scope: &SyntaxNode,
    insert_path: ast::Path,
) -> (InsertPosition<SyntaxElement>, AddBlankLine) {
    let group = ImportGroup::new(&insert_path);
    let path_node_iter = scope
        .children()
        .filter_map(|node| ast::Use::cast(node.clone()).zip(Some(node)))
        .flat_map(|(use_, node)| use_.use_tree().and_then(|tree| tree.path()).zip(Some(node)));
    // Iterator that discards anything thats not in the required grouping
    // This implementation allows the user to rearrange their import groups as this only takes the first group that fits
    let group_iter = path_node_iter
        .clone()
        .skip_while(|(path, _)| ImportGroup::new(path) != group)
        .take_while(|(path, _)| ImportGroup::new(path) == group);

    let segments = segment_iter(&insert_path);
    // track the last element we iterated over, if this is still None after the iteration then that means we never iterated in the first place
    let mut last = None;
    // find the element that would come directly after our new import
    let post_insert =
        group_iter.inspect(|(_, node)| last = Some(node.clone())).find(|(path, _)| {
            let check_segments = segment_iter(&path);
            segments
                .clone()
                .zip(check_segments)
                .flat_map(|(seg, seg2)| seg.name_ref().zip(seg2.name_ref()))
                .all(|(l, r)| l.text() <= r.text())
        });
    match post_insert {
        // insert our import before that element
        Some((_, node)) => (InsertPosition::Before(node.into()), AddBlankLine::After),
        // there is no element after our new import, so append it to the end of the group
        None => match last {
            Some(node) => (InsertPosition::After(node.into()), AddBlankLine::Before),
            // the group we were looking for actually doesnt exist, so insert
            None => {
                // similar concept here to the `last` from above
                let mut last = None;
                // find the group that comes after where we want to insert
                let post_group = path_node_iter
                    .inspect(|(_, node)| last = Some(node.clone()))
                    .find(|(p, _)| ImportGroup::new(p) > group);
                match post_group {
                    Some((_, node)) => {
                        (InsertPosition::Before(node.into()), AddBlankLine::AfterTwice)
                    }
                    // there is no such group, so append after the last one
                    None => match last {
                        Some(node) => (InsertPosition::After(node.into()), AddBlankLine::Before),
                        // there are no imports in this file at all
                        None => (InsertPosition::First, AddBlankLine::AfterTwice),
                    },
                }
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use test_utils::assert_eq_text;

    #[test]
    fn insert_start() {
        check_none(
            "std::bar::A",
            r"use std::bar::B;
use std::bar::D;
use std::bar::F;
use std::bar::G;",
            r"use std::bar::A;
use std::bar::B;
use std::bar::D;
use std::bar::F;
use std::bar::G;",
        )
    }

    #[test]
    fn insert_middle() {
        check_none(
            "std::bar::E",
            r"use std::bar::A;
use std::bar::D;
use std::bar::F;
use std::bar::G;",
            r"use std::bar::A;
use std::bar::D;
use std::bar::E;
use std::bar::F;
use std::bar::G;",
        )
    }

    #[test]
    fn insert_end() {
        check_none(
            "std::bar::Z",
            r"use std::bar::A;
use std::bar::D;
use std::bar::F;
use std::bar::G;",
            r"use std::bar::A;
use std::bar::D;
use std::bar::F;
use std::bar::G;
use std::bar::Z;",
        )
    }

    #[test]
    fn insert_middle_pnested() {
        check_none(
            "std::bar::E",
            r"use std::bar::A;
use std::bar::{D, Z}; // example of weird imports due to user
use std::bar::F;
use std::bar::G;",
            r"use std::bar::A;
use std::bar::E;
use std::bar::{D, Z}; // example of weird imports due to user
use std::bar::F;
use std::bar::G;",
        )
    }

    #[test]
    fn insert_middle_groups() {
        check_none(
            "foo::bar::G",
            r"use std::bar::A;
use std::bar::D;

use foo::bar::F;
use foo::bar::H;",
            r"use std::bar::A;
use std::bar::D;

use foo::bar::F;
use foo::bar::G;
use foo::bar::H;",
        )
    }

    #[test]
    fn insert_first_matching_group() {
        check_none(
            "foo::bar::G",
            r"use foo::bar::A;
use foo::bar::D;

use std;

use foo::bar::F;
use foo::bar::H;",
            r"use foo::bar::A;
use foo::bar::D;
use foo::bar::G;

use std;

use foo::bar::F;
use foo::bar::H;",
        )
    }

    #[test]
    fn insert_missing_group() {
        check_none(
            "std::fmt",
            r"use foo::bar::A;
use foo::bar::D;",
            r"use std::fmt;

use foo::bar::A;
use foo::bar::D;",
        )
    }

    #[test]
    fn insert_no_imports() {
        check_full(
            "foo::bar",
            "fn main() {}",
            r"use foo::bar;

fn main() {}",
        )
    }

    #[test]
    fn insert_empty_file() {
        // empty files will get two trailing newlines
        // this is due to the test case insert_no_imports above
        check_full(
            "foo::bar",
            "",
            r"use foo::bar;

",
        )
    }

    #[test]
    fn adds_std_group() {
        check_full(
            "std::fmt::Debug",
            r"use stdx;",
            r"use std::fmt::Debug;

use stdx;",
        )
    }

    #[test]
    fn merges_groups() {
        check_last("std::io", r"use std::fmt;", r"use std::{fmt, io};")
    }

    #[test]
    fn merges_groups_last() {
        check_last(
            "std::io",
            r"use std::fmt::{Result, Display};",
            r"use std::fmt::{Result, Display};
use std::io;",
        )
    }

    #[test]
    fn merges_groups2() {
        check_full(
            "std::io",
            r"use std::fmt::{Result, Display};",
            r"use std::{fmt::{Result, Display}, io};",
        )
    }

    #[test]
    fn skip_merges_groups_pub() {
        check_full(
            "std::io",
            r"pub use std::fmt::{Result, Display};",
            r"pub use std::fmt::{Result, Display};
use std::io;",
        )
    }

    #[test]
    fn merges_groups_self() {
        check_full("std::fmt::Debug", r"use std::fmt;", r"use std::fmt::{self, Debug};")
    }

    fn check(
        path: &str,
        ra_fixture_before: &str,
        ra_fixture_after: &str,
        mb: Option<MergeBehaviour>,
    ) {
        let file = ast::SourceFile::parse(ra_fixture_before).tree().syntax().clone();
        let path = ast::SourceFile::parse(&format!("use {};", path))
            .tree()
            .syntax()
            .descendants()
            .find_map(ast::Path::cast)
            .unwrap();

        let result = insert_use(file, path, mb).to_string();
        assert_eq_text!(&result, ra_fixture_after);
    }

    fn check_full(path: &str, ra_fixture_before: &str, ra_fixture_after: &str) {
        check(path, ra_fixture_before, ra_fixture_after, Some(MergeBehaviour::Full))
    }

    fn check_last(path: &str, ra_fixture_before: &str, ra_fixture_after: &str) {
        check(path, ra_fixture_before, ra_fixture_after, Some(MergeBehaviour::Last))
    }

    fn check_none(path: &str, ra_fixture_before: &str, ra_fixture_after: &str) {
        check(path, ra_fixture_before, ra_fixture_after, None)
    }
}
