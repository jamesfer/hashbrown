use hashbrown::raw::raw_table_double_hash::RawTableDoubleHash;
use hashbrown::raw::RawTable;
use hashbrown_hashjoin::new_map_3::fixed_table::{ReadOnlyFixedTable, WritableFixedTable};
use hashbrown_hashjoin::new_map_3::group::Group8;

pub fn main() {
    let table = WritableFixedTable::<_, _>::with_capacity(16);
    table.insert(100, 200).unwrap();

    let mut raw = RawTable::with_capacity(16);
    raw.insert(100, (100, 200), |(hash, _)| *hash);

    let mut raw_d = RawTableDoubleHash::with_capacity(16);
    raw_d.insert(100, (100, 200), |(hash, _)| *hash);

    let table1 = table.to_read_only();
    my_table(&table1, 100);
    my_table_bulk_group(&table1, &[100, 200, 300, 400, 500, 600, 700, 800]);
    raw_table(&raw, 100, 200);
    raw_table_d(&raw_d, 100, 200);
}

#[inline(never)]
#[no_mangle]
fn my_table(read_table: &ReadOnlyFixedTable<usize, Group8>, h: u64) -> Option<usize> {
    read_table.get(h).copied()
}

#[inline(never)]
#[no_mangle]
fn raw_table(table: &RawTable<(u64, usize)>, h: u64, n: usize) -> Option<(u64, usize)> {
    table.get(h, |(_, v)| *v == n).copied()
}

#[inline(never)]
#[no_mangle]
fn raw_table_d(table: &RawTableDoubleHash<(u64, usize)>, h: u64, n: usize) -> Option<(u64, usize)> {
    table.get(h, |(_, v)| *v == n).copied()
}

#[inline(never)]
#[no_mangle]
fn my_table_bulk_group<'a>(table: &'a ReadOnlyFixedTable<usize, Group8>, group: &'a [u64; 8]) -> [Option<&'a usize>; 8] {
    table.get_in_bulk_static_8(group)
}
