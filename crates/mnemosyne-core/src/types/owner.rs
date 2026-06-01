/// Permission identity for the thread allocator that owns a segment.
///
/// This follows the GhostCell separation principle at allocator scale: segment
/// data stores an opaque ownership token, while mutation permission remains with
/// the thread-local allocator that can prove token equality. The representation
/// is a raw pointer-sized value, so checks compile to the same pointer
/// comparison as the previous untyped field.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SegmentOwner(pub usize);

impl SegmentOwner {
    /// No thread currently owns this segment.
    pub const NONE: Self = Self(0);

    /// Builds an owner token from an allocator pointer.
    #[inline(always)]
    pub fn from_ptr<T>(ptr: *mut T) -> Self {
        Self(ptr as usize)
    }

    /// Returns true when this token identifies `ptr`.
    #[inline(always)]
    pub fn matches<T>(self, ptr: *mut T) -> bool {
        self.0 == ptr as usize
    }

    /// Builds an owner token from a thread ID.
    #[inline(always)]
    pub fn from_thread_id(tid: u32) -> Self {
        Self(tid as usize)
    }

    /// Returns true when this token identifies `tid`.
    #[inline(always)]
    pub fn matches_thread_id(self, tid: u32) -> bool {
        self.0 == tid as usize
    }
}

unsafe impl Send for SegmentOwner {}
unsafe impl Sync for SegmentOwner {}
