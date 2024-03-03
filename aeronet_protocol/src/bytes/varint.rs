pub const fn size_of(mut v: u64) -> usize {
    if v == 0 {
        return 1;
    }
    let mut size = 0;
    while v > 0 {
        size += 1;
        v >>= 7;
    }
    size
}
