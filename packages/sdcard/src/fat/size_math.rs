fn clusters_for_size(size: usize, cluster_size: usize) -> usize {
    if size == 0 {
        0
    } else {
        (size - 1) / cluster_size + 1
    }
}
