"""Copy the unchanged complete product workload and add separate DHAT counters."""
from pathlib import Path
import hashlib,io,json,subprocess,tarfile
root=Path(__file__).resolve().parent;repo=Path.cwd();frozen=root/'baseline-source';frozen.mkdir(exist_ok=True)
archive=subprocess.check_output(['git','archive','3803549','Cargo.toml','Cargo.lock','crates'])
with tarfile.open(fileobj=io.BytesIO(archive)) as t:
 t.extractall(frozen)
source=(repo/'integration/egui-product-harness/examples/product_cost.rs').read_text()
original=source
replacements=[
 ('use std::time::Instant;', '''use std::time::Instant;

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[derive(Clone, Copy)]
struct Allocation {
    events: u64,
    bytes: u64,
    live_before: usize,
    live_after: usize,
    prefix_peak: usize,
}
'''),
 ('draw_commands";', 'draw_commands,allocation_events,requested_bytes,live_before_bytes,live_after_bytes,case_prefix_peak_bytes";'),
 ('struct Frame {\n    elapsed_ns: u128,', 'struct Frame {\n    allocation: Allocation,\n    elapsed_ns: u128,'),
 ('        let started = Instant::now();','        let heap_before = dhat::HeapStats::get();\n        let started = Instant::now();'),
 ('        let elapsed_ns = started.elapsed().as_nanos();','''        let elapsed_ns = started.elapsed().as_nanos();
        let heap_after = dhat::HeapStats::get();
        let allocation = Allocation {
            events: heap_after.total_blocks - heap_before.total_blocks,
            bytes: heap_after.total_bytes - heap_before.total_bytes,
            live_before: heap_before.curr_bytes,
            live_after: heap_after.curr_bytes,
            prefix_peak: heap_after.max_bytes,
        };'''),
 ('        Ok(Frame {\n            elapsed_ns,','        Ok(Frame {\n            allocation,\n            elapsed_ns,'),
 ('fn sample(&mut self, scenario: Scenario) -> Result<(u128, Counts)>','fn sample(&mut self, scenario: Scenario) -> Result<(u128, Counts, Allocation)>'),
 ('        let mut elapsed_ns = first.elapsed_ns;','        let mut elapsed_ns = first.elapsed_ns;\n        let mut allocation = first.allocation;'),
 ('            elapsed_ns += second.elapsed_ns;','''            elapsed_ns += second.elapsed_ns;
            allocation.events += second.allocation.events;
            allocation.bytes += second.allocation.bytes;
            allocation.live_after = second.allocation.live_after;
            allocation.prefix_peak = allocation.prefix_peak.max(second.allocation.prefix_peak);'''),
 ('        Ok((elapsed_ns, counts))','        Ok((elapsed_ns, counts, allocation))'),
 ('            let mut harness = Harness::new(count, scenario)?;','''            let _profiler = dhat::Profiler::builder().testing().trim_backtraces(Some(4)).build();
            let mut harness = Harness::new(count, scenario)?;'''),
 ('let (elapsed_ns, counts) = harness.sample(scenario)', 'let (elapsed_ns, counts, allocation) = harness.sample(scenario)'),
 ('{},{},{},{},{},{}",\n                    scenario.name()', '{},{},{},{},{},{},{},{},{},{},{}",\n                    scenario.name()'),
 ('                    counts.draw_commands,','''                    counts.draw_commands,
                    allocation.events,
                    allocation.bytes,
                    allocation.live_before,
                    allocation.live_after,
                    allocation.prefix_peak,'''),
]
for old,new in replacements:
 assert source.count(old)==1,(old,source.count(old))
 source=source.replace(old,new)
patches='\n'.join(f'{old!r} => {new!r}' for old,new in replacements)
(root/'allocation-instrumentation.json').write_text(json.dumps({'workload_source':'integration/egui-product-harness/examples/product_cost.rs','original_sha256':hashlib.sha256(original.encode()).hexdigest(),'instrumented_sha256':hashlib.sha256(source.encode()).hexdigest(),'replacements':replacements},indent=2)+'\n')
patches=(repo/'integration/egui-fork-workspace/Cargo.toml').read_text().split('[patch.crates-io]',1)[1]
for label,libroot in [('baseline',frozen),('candidate',repo)]:
 folder=root/f'allocation-{label}';(folder/'src').mkdir(parents=True)
 (folder/'src/main.rs').write_text(source)
 manifest=f'''[workspace]
resolver = "3"
[package]
name = "dockspace-product-allocation-probe"
version = "0.0.0"
edition = "2024"
[dependencies]
dockspace = {{ path = "{libroot}/crates/dockspace", default-features = false }}
egui_dockspace = {{ path = "{libroot}/crates/egui_dockspace", default-features = false }}
egui = {{ version = "=0.37.0", default-features = false, features = ["default_fonts"] }}
dhat = "=0.3.3"
[profile.production]
inherits = "release"
lto = "thin"
codegen-units = 1
[lints.rust]
unsafe_code = "forbid"
[patch.crates-io]
{patches}
'''
 (folder/'Cargo.toml').write_text(manifest)
print('Prepared two external probes; identical instrumentation, exact frozen baseline source, no repository mutation')
