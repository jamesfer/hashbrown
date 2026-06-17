use criterion::{Bencher, Criterion};
use criterion::measurement::Measurement;

pub trait DefinesBenchmark<M>
where
    M: Measurement + 'static,
{
    fn bench_function<F>(&mut self, id: &str, f: F)
    where F: FnMut(&mut Bencher<'_, M>);
}
