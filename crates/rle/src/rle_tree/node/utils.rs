/// distribute the num to a array, where the sum of the array is num
/// and each element is in the range [min, max]
pub(super) fn distribute(mut num: usize, min: usize, max: usize) -> Vec<usize> {
    if num <= max {
        return vec![num];
    }

    let n = num / min;
    let mut arr = vec![min; n];
    num -= n * min;
    while num > 0 {
        for value in arr.iter_mut() {
            if num == 0 {
                break;
            }
            if *value < max {
                *value += 1;
                num -= 1;
            }
        }
    }

    arr
}
