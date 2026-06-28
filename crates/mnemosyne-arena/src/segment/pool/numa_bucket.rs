use themis::NumaNodeId;

pub(crate) const NUMA_BUCKETS: usize = 16;

#[inline(always)]
pub(crate) fn bucket_from_u32(node: u32) -> usize {
    NumaNodeId::new(node).bucket_index::<NUMA_BUCKETS>().index()
}

#[inline(always)]
pub(crate) fn bucket_from_usize(node: usize) -> usize {
    bucket_from_u32(node as u32)
}

#[inline]
pub(crate) fn steal_from<T>(
    start_node: usize,
    mut pop_from_node: impl FnMut(usize) -> Option<T>,
) -> Option<T> {
    let start = NumaNodeId::new(start_node as u32).bucket_index::<NUMA_BUCKETS>();
    for offset in 1..NUMA_BUCKETS {
        if let Some(value) = pop_from_node(start.wrapping_add(offset).index()) {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steal_from_visits_every_nonlocal_bucket_once_in_wrap_order() {
        let mut visited = Vec::new();
        let result: Option<()> = steal_from(14, |node| {
            visited.push(node);
            None
        });

        assert!(result.is_none());
        assert_eq!(
            visited,
            vec![15, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]
        );
    }

    #[test]
    fn steal_from_stops_on_first_hit() {
        let mut visited = Vec::new();
        let result = steal_from(2, |node| {
            visited.push(node);
            (node == 5).then_some(node)
        });

        assert_eq!(result, Some(5));
        assert_eq!(visited, vec![3, 4, 5]);
    }
}
