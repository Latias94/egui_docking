"""Apply or remove one experiment's exact, temporary measurement spans."""

from pathlib import Path
import hashlib
import json
import subprocess
import sys

root = Path(__file__).resolve().parent
repo = Path.cwd()
manifest = root/'instrumentation.json'
if sys.argv[1]=='remove':
    data=json.loads(manifest.read_text())
    for name,row in data.items():
        assert hashlib.sha256((repo/name).read_bytes()).hexdigest()==row['instrumented'], name
    for name,row in data.items():
        (repo/name).write_bytes((root/'originals'/name).read_bytes())
    print('Removed exact temporary changes from',len(data),'files; original bytes restored')
    raise SystemExit(0)

files={}

def load(name):
    if name not in files:
        dirty=subprocess.run(['git','diff','--name-only','HEAD','--',name],capture_output=True,text=True,check=True).stdout
        assert not dirty,name
        files[name]=(repo/name).read_text()
    return files[name]

def replace(name,old,new):
    text=load(name)
    assert text.count(old)==1,(name,old,text.count(old))
    files[name]=text.replace(old,new,1)

def function(name,function,label):
    text=load(name)
    start=text.index('fn '+function+'(')
    opening=text.index('{',start)
    files[name]=text[:opening+1]+f'\n        let mut selection_cost_probe = crate::selection_cost_probe::Span::new("{label}");'+text[opening+1:]

lib='crates/dockspace/src/lib.rs'
files[lib]=load(lib)+f'\n#[path = "{root}/probe.rs"]\nmod selection_cost_probe;\n'

operation='crates/dockspace/src/operation.rs'
function(operation,'prepare_transaction_inner','transaction')
replace(operation,'    let mut candidate = clone_workspace_draft(workspace);\n    // Typed command deltas','    let mut candidate = clone_workspace_draft(workspace);\n    selection_cost_probe.mark("clone");\n    // Typed command deltas')
replace(operation,'    let mut item_ledger = workspace.item_multiset();','    let mut item_ledger = workspace.item_multiset();\n    selection_cost_probe.mark("initial_item_ledger");')
replace(operation,'    prune_vacant_surfaces(&mut candidate);\n    canonicalize_draft(&mut candidate)?;\n    candidate.validate()?;','    selection_cost_probe.mark("commands");\n    prune_vacant_surfaces(&mut candidate);\n    canonicalize_draft(&mut candidate)?;\n    selection_cost_probe.mark("canonicalize");\n    candidate.validate()?;\n    selection_cost_probe.mark("validate");')
replace(operation,'    let actual_items = candidate.item_multiset();\n    if actual_items != item_ledger {','    let actual_items = candidate.item_multiset();\n    selection_cost_probe.mark("reconcile");\n    if actual_items != item_ledger {')
function(operation,'select','select_edit')
replace(operation,'Span::new("select_edit");\n    validate_item_source(workspace, source)?;','Span::new("select_edit");\n    validate_item_source(workspace, source)?;\n    selection_cost_probe.mark("source_validation");')
replace(operation,'    promote_tab_mru(workspace, tabs, item)?;\n    Ok(AppliedCommand {','    promote_tab_mru(workspace, tabs, item)?;\n    selection_cost_probe.mark("edit_and_mru");\n    Ok(AppliedCommand {')
function('crates/dockspace/src/transaction.rs','into_parts','transaction_publication')
authority='crates/dockspace/src/engine/presentation_authority.rs'
function(authority,'try_rebuild_presentation_requirements','requirements_refresh')
function(authority,'capture_surface_semantic_snapshots','surface_snapshots')
requirements='crates/dockspace/src/scene_compiler/requirements.rs'
function(requirements,'derive_scene_requirement_draft','requirements_index_and_draft')
function(requirements,'derive_scene_requirement_draft_with_index','requirements_draft')
function('crates/dockspace/src/workspace.rs','build','workspace_index')
function('crates/dockspace/src/scene_compiler/surface.rs','compile_surface_scene','scene_compile')
validation='crates/dockspace/src/scene/validation.rs'
function(validation,'validate_and_canonicalize','scene_validate')
replace(validation,'        let drop_target_index = validate_semantic_uniqueness(&ready)?;','        let drop_target_index = validate_semantic_uniqueness(&ready)?;\n        selection_cost_probe.mark("uniqueness");')
replace(validation,'        let published_occlusions: HashSet<_> = ready','        selection_cost_probe.mark("semantics");\n        let published_occlusions: HashSet<_> = ready')
replace(validation,'        let target_order = drop_target_index.into_order();','        selection_cost_probe.mark("targets");\n        let target_order = drop_target_index.into_order();')
replace(validation,'        if ready.measurement_ticket().is_some() {\n            ready.link_surface_program()?;','        selection_cost_probe.mark("canonicalize");\n        if ready.measurement_ticket().is_some() {\n            ready.link_surface_program()?;')
lifecycle='crates/dockspace/src/scene/lifecycle.rs'
function(lifecycle,'install_ready','scene_install')
replace(lifecycle,'        let hit_manifest = PresentationHitManifest::compile_reusing(','        selection_cost_probe.mark("before_hit");\n        let hit_manifest = PresentationHitManifest::compile_reusing(')
replace(lifecycle,'        let paint_program_valid = validate_surface_paint_program(&plan, &hit_manifest);','        selection_cost_probe.mark("hit");\n        let paint_program_valid = validate_surface_paint_program(&plan, &hit_manifest);\n        selection_cost_probe.mark("paint_valid");')
replace(lifecycle,'        let semantic_manifest = PresentationSemanticManifest::compile(output_ticket, &plan)?;','        let semantic_manifest = PresentationSemanticManifest::compile(output_ticket, &plan)?;\n        selection_cost_probe.mark("semantic");')
function('crates/dockspace/src/engine.rs','candidate','engine_candidate')

measurement='crates/dockspace/src/runtime/measurement.rs'
function(measurement,'measure_surface_with','measurement')
replace(measurement,'        self.complete_pointer_journal()?;','        self.complete_pointer_journal()?;\n        selection_cost_probe.mark("input_close");')
replace(measurement,'        let token = self.frame.view().begin_surface_contribution(surface)?;\n        let contribution = self\n            .frame\n            .view()\n            .prepare_surface_contribution(token, measurements)?;\n        self.frame.compile_surface_contribution(contribution)?;', '        selection_cost_probe.mark("answer_assembly");\n        let token = self.frame.view().begin_surface_contribution(surface)?;\n        let contribution = self\n            .frame\n            .view()\n            .prepare_surface_contribution(token, measurements)?;\n        selection_cost_probe.mark("contribution_prepare");\n        self.frame.compile_surface_contribution(contribution)?;\n        selection_cost_probe.mark("contribution_compile");')

harness='integration/dockspace-host-harness/examples/frame_cost.rs'
replace(harness,'const SURFACE: SurfaceId = SurfaceId::new(1);',f'#[path = "{root}/probe.rs"]\nmod selection_cost_probe;\n\nconst SURFACE: SurfaceId = SurfaceId::new(1);')
replace(harness,'    fn update(&mut self, scenario: Scenario, pointer_event: Option<SurfacePointerEvent>) {','    fn update(&mut self, scenario: Scenario, pointer_event: Option<SurfacePointerEvent>) {\n        eprintln!("[DEBUG-select-cost] begin_update {} {} {}", scenario.name(), self.panes, self.step);\n        let mut selection_cost_probe = selection_cost_probe::Span::new("harness_update");')
replace(harness,'        if matches!(scenario, Scenario::Select | Scenario::OutputsInFlight) {','        selection_cost_probe.mark("begin_frame");\n        if matches!(scenario, Scenario::Select | Scenario::OutputsInFlight) {')
replace(harness,'        if let Some((event, point, hit)) = pointer {','        selection_cost_probe.mark("submit_selection");\n        if let Some((event, point, hit)) = pointer {')
replace(harness,'        let changed_measurement = matches!(','        selection_cost_probe.mark("pointer");\n        let changed_measurement = matches!(')
replace(harness,'        let plan = match frame.surface_work(SURFACE).unwrap() {','        selection_cost_probe.mark("measure");\n        let plan = match frame.surface_work(SURFACE).unwrap() {')
replace(harness,'        let (sink, receipt) = RecordingSink::encode(plan);','        selection_cost_probe.mark("surface_work");\n        let (sink, receipt) = RecordingSink::encode(plan);\n        selection_cost_probe.mark("encode");')
replace(harness,'        frame.confirm_surface_painted(receipt).unwrap();','        frame.confirm_surface_painted(receipt).unwrap();\n        selection_cost_probe.mark("confirm_paint");')
replace(harness,'        let mut report = frame.commit().unwrap();','        let mut report = frame.commit().unwrap();\n        selection_cost_probe.mark("commit");')
replace(harness,'        self.step += 1;\n    }','        self.step += 1;\n        eprintln!("[DEBUG-select-cost] end_update");\n    }')

data={}
for name,text in files.items():
    original=(repo/name).read_bytes()
    saved=root/'originals'/name;saved.parent.mkdir(parents=True,exist_ok=True);saved.write_bytes(original)
    data[name]={'original':hashlib.sha256(original).hexdigest(),'instrumented':hashlib.sha256(text.encode()).hexdigest()}
for name,text in files.items():
    (repo/name).write_text(text)
manifest.write_text(json.dumps(data,indent=2)+'\n')
print('Instrumented',len(data),'files for one non-shipping diagnostic executable')
