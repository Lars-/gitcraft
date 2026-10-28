//! How fast GitCraft reads and lays out history.
//!
//! The synthetic benchmarks always run. Point `GITCRAFT_BENCH_REPO` at a large clone (CI uses
//! git.git) to also measure a real repository.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use gitcraft_core::graph::{Graph, Parents};
use gitcraft_core::repo::Repo;

/// A history with many long-lived branches that keep merging into each other.
fn tangled(commits: usize, branches: usize) -> Parents {
    let mut parents = Parents::default();
    for i in 0..commits {
        let next = (i + branches..commits).step_by(branches).take(1);
        let merge = (i % 7 == 0).then_some(i + 3).filter(|&p| p < commits);
        parents.push(next.chain(merge));
    }
    parents
}

fn graph(c: &mut Criterion) {
    let parents = tangled(200_000, 12);
    c.bench_function("layout 200k commits", |b| b.iter(|| Graph::new(black_box(parents.clone()))));
    let graph = Graph::new(parents);
    c.bench_function("rows for ten screens", |b| b.iter(|| graph.rows(black_box(150_000..150_600))));
}

fn repository(c: &mut Criterion) {
    let Some(path) = std::env::var_os("GITCRAFT_BENCH_REPO") else { return };
    let repo = Repo::open(path).expect("GITCRAFT_BENCH_REPO is a repository");
    c.bench_function("history of the bench repository", |b| b.iter(|| repo.history().unwrap()));
    let history = repo.history().unwrap();
    let screens = &history.ids[history.ids.len() / 2..][..600];
    c.bench_function("commits for ten screens", |b| {
        b.iter(|| screens.iter().map(|&id| repo.commit(id).unwrap()).collect::<Vec<_>>());
    });
}

criterion_group!(benches, graph, repository);
criterion_main!(benches);
