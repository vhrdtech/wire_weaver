use crate::ast::docs::Docs;
use crate::ast::object_size::ObjectSize;

pub(crate) fn add_notes(docs: &mut Docs, size_assumption: Option<ObjectSize>, is_enum: bool) {
    docs.push_str("");
    match size_assumption {
        Some(ObjectSize::Unsized) | None => {
            docs.push_str("NOTE(shrink_wrap): Unsized: This type can be evolved with backwards and forwards compatibility.");
        }
        Some(ObjectSize::UnsizedFinalStructure)
        | Some(ObjectSize::Sized { .. })
        | Some(ObjectSize::SelfDescribing) => {
            docs.push_str(
                "NOTE(shrink_wrap): Structure of this type can no longer be changed without breaking compatibility,",
            );
            docs.push_str(
                "NOTE(shrink_wrap): only reserved bits can still be used to carry new information (if any).",
            );
        }
    }
    if is_enum {
        docs.push_str("Enum variants can be added if there is space left and if code already in use can handle them.")
    }
}
