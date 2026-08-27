//! Page-local allocation and intrusive-list concerns.

mod allocation;
mod lists;
mod transitions;

pub(crate) use allocation::{
    pop_page_free_block, try_allocate_page_local, try_reclaim_and_allocate,
};
pub(crate) use lists::{
    move_page_between_lists_branded, push_page_front, unlink_page_from_list, with_page_list_token,
};
