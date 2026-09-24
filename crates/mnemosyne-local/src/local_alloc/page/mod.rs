//! Page-local allocation and intrusive-list concerns.

mod allocation;
mod lists;
mod transitions;

pub(crate) use allocation::{
    try_allocate_page_local, try_allocate_page_local_dynamic, try_reclaim_and_allocate,
    try_reclaim_and_allocate_dynamic,
};
pub(crate) use lists::{move_page_raw, push_page_front_raw, unlink_page_from_list_raw};
