use std::sync::Arc;

pub trait Lookup {
    fn get(&self, hash: u64) -> Vec<usize>;
}

impl <L> Lookup for Arc<L>
where
    L: Lookup
{
    fn get(&self, hash: u64) -> Vec<usize> {
        self.as_ref().get(hash)
    }
}
