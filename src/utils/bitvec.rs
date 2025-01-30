use bitvec::{order::Msb0, vec::BitVec};
use rand::Rng;

pub fn choose_random_bit_indices(bitvec: &BitVec<u8, Msb0>, num_indices: usize) -> Vec<usize> {
    let set_indices: Vec<usize> = bitvec
        .iter()
        .enumerate()
        .filter_map(|(i, bit)| if *bit { Some(i) } else { None })
        .collect();

    if set_indices.is_empty() {
        return Vec::new(); // Handle case where no bits are set
    }

    let num_to_choose = std::cmp::min(num_indices, set_indices.len()); // Don't try to choose more than are available
    let mut rng = rand::thread_rng();
    let mut chosen_indices: Vec<usize> = Vec::with_capacity(num_to_choose);
    let mut chosen_set = std::collections::HashSet::new();

    while chosen_indices.len() < num_to_choose {
        let random_index_within_set = rng.gen_range(0..set_indices.len());
        let actual_index = set_indices[random_index_within_set]; // Get the actual index from the set_indices

        if !chosen_set.contains(&actual_index) {
            chosen_indices.push(actual_index);
            chosen_set.insert(actual_index);
        }
    }

    chosen_indices
}


pub fn pieces_amount_to_bv(amount: u32) -> BitVec<u8, Msb0> {
    let num_bytes  = (amount as f32 / 8.0).ceil() as usize;
    let mut bitfield: BitVec<u8, Msb0> = BitVec::with_capacity(num_bytes * 8);
    
    for _ in 0..amount {
        bitfield.push(true);
    }

    bitfield
}