//! Construction and linked traversal of the single immutable surface program.

use super::*;

#[cfg(test)]
#[path = "program/scratch_tests.rs"]
mod scratch_tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceProgramRootPresentation {
    Main,
    Contained(FloatingPresentationId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SurfaceProgramRootOperation {
    TabBar(TabBarSceneId),
    Tab(TabSceneId),
    Pane(PaneSceneId),
    Splitter(SplitterSceneId),
    SplitterJunction(SplitterJunctionId),
    TabStripControl(TabStripControlId),
    ContainedControls(FloatingPresentationId),
    PresentationMenuAnchor(RootId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SurfaceProgramBlock {
    RootBegin {
        root: RootId,
        presentation: SurfaceProgramRootPresentation,
        title_item: Option<ItemId>,
    },
    RootOperation(SurfaceProgramRootOperation),
    RootEnd {
        root: RootId,
    },
    PopupBackdrop(TabListMenuSessionId),
    PopupMenu(TabListMenuSessionId),
    DragPreview,
    DropTarget(DropTargetId),
    DropGuide(DropGuideClusterId),
    ContainedTransformPreview,
    DragDecoration,
    InteractionFeedback,
    SurfaceEnd,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResolvedSurfaceProgramRoot<'a> {
    root: RootId,
    contained: Option<&'a ContainedRecord>,
    title_item: Option<ItemId>,
}

impl<'a> ResolvedSurfaceProgramRoot<'a> {
    pub(crate) const fn root(self) -> RootId {
        self.root
    }

    pub(crate) const fn contained_record(self) -> Option<&'a ContainedRecord> {
        self.contained
    }

    pub(crate) const fn title_item(self) -> Option<ItemId> {
        self.title_item
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ResolvedSurfaceProgramOperation<'a> {
    Pane(&'a PaneRecord),
    TabBar(&'a TabBarRecord),
    Tab(&'a TabRecord),
    Splitter(&'a SplitterRecord),
    SplitterJunction(&'a SplitterJunctionRecord),
    TabStripControl(&'a TabStripControlRecord),
    ContainedControls(&'a ContainedRecord),
    PresentationMenuAnchor(&'a PresentationMenuAnchorRecord),
}

#[allow(
    dead_code,
    reason = "the common visitor carries root provenance for the renderer sink"
)]
#[derive(Debug, Clone, Copy)]
pub(crate) enum ResolvedSurfaceProgramStep<'a> {
    RootBegin(ResolvedSurfaceProgramRoot<'a>),
    Operation {
        root: ResolvedSurfaceProgramRoot<'a>,
        operation: ResolvedSurfaceProgramOperation<'a>,
    },
    RootEnd(ResolvedSurfaceProgramRoot<'a>),
    PopupBackdrop(&'a TabListMenuBackdropRecord),
    PopupMenu(&'a TabListMenuRecord),
    DragPreview,
    DropTarget(&'a DropTargetRecord),
    DropGuide(&'a DropGuideClusterRecord),
    ContainedTransformPreview,
    DragDecoration,
    InteractionFeedback,
    SurfaceEnd,
}

#[derive(Debug)]
pub(crate) enum SurfaceProgramVisitError<E> {
    InvalidProgram,
    Sink(E),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum SurfaceProgramBuildError {
    #[error("surface program already has an open root")]
    RootAlreadyOpen,
    #[error("surface program has no open root")]
    RootNotOpen,
    #[error("surface program expected open root {expected}, found {actual}")]
    RootMismatch { expected: RootId, actual: RootId },
    #[error("surface program already contains root {root}")]
    DuplicateRoot { root: RootId },
    #[error("surface program does not contain root {root}")]
    MissingRoot { root: RootId },
    #[error("surface program was already finished")]
    AlreadyFinished,
    #[error("surface program repeats operation {operation:?} for root {root}")]
    DuplicateOperation {
        root: RootId,
        operation: SurfaceProgramRootOperation,
    },
    #[error("surface program repeats guide cluster {id:?}")]
    DuplicateGuide { id: DropGuideClusterId },
    #[error("surface program repeats popup backdrop {session:?}")]
    DuplicatePopupBackdrop { session: TabListMenuSessionId },
    #[error("surface program repeats popup menu {session:?}")]
    DuplicatePopupMenu { session: TabListMenuSessionId },
    #[error("surface program popup session {session:?} has no backdrop")]
    PopupMenuWithoutBackdrop { session: TabListMenuSessionId },
    #[error("surface program repeats drop target {id:?}")]
    DuplicateDropTarget { id: DropTargetId },
}

// Slots are meaningful only inside the immutable record tables they were linked against.
macro_rules! record_rows {
    ($($name:ident),+ $(,)?) => {
        $(#[derive(Debug, Clone, Copy, PartialEq, Eq)]
        struct $name(usize);)+
    };
}

record_rows!(
    PaneRow,
    TabBarRow,
    TabRow,
    SplitterRow,
    SplitterJunctionRow,
    TabStripControlRow,
    ContainedRow,
    PresentationMenuAnchorRow,
    PopupBackdropRow,
    PopupMenuRow,
    DropTargetRow,
    DropGuideRow,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LinkedSurfaceProgramRoot {
    root: RootId,
    contained: Option<ContainedRow>,
    title_item: Option<ItemId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkedSurfaceProgramOperation {
    Pane(PaneRow),
    TabBar(TabBarRow),
    Tab(TabRow),
    Splitter(SplitterRow),
    SplitterJunction(SplitterJunctionRow),
    TabStripControl(TabStripControlRow),
    ContainedControls(ContainedRow),
    PresentationMenuAnchor(PresentationMenuAnchorRow),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkedSurfaceProgramDropTarget {
    Background,
    Record(DropTargetRow),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SurfaceProgramInstruction {
    RootBegin(LinkedSurfaceProgramRoot),
    RootOperation(LinkedSurfaceProgramOperation),
    RootEnd(RootId),
    PopupBackdrop(PopupBackdropRow),
    PopupMenu(PopupMenuRow),
    DragPreview,
    DropTarget(LinkedSurfaceProgramDropTarget),
    DropGuide(DropGuideRow),
    ContainedTransformPreview,
    DragDecoration,
    InteractionFeedback,
    SurfaceEnd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SurfaceProgramTape {
    Draft(Vec<SurfaceProgramBlock>),
    Linked(Box<[SurfaceProgramInstruction]>),
}

impl Default for SurfaceProgramTape {
    fn default() -> Self {
        Self::Draft(Vec::new())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SurfaceProgramPhase {
    Roots,
    Targets,
    Guides,
    ContainedPreview,
    Decoration,
    PopupBackdrops,
    PopupMenus,
    Feedback,
    End,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SurfaceProgram {
    tape: SurfaceProgramTape,
    root_drafts: Vec<SurfaceProgramRootDraft>,
    root_indices: std::collections::BTreeMap<RootId, usize>,
    active_root: Option<usize>,
    deferred_popup_backdrops: Vec<TabListMenuSessionId>,
    deferred_popup_menus: Vec<TabListMenuSessionId>,
    deferred_drop_targets: Vec<DropTargetId>,
    deferred_surface_background: Option<DropTargetId>,
    deferred_guides: Vec<DropGuideClusterId>,
    deferred_guide_targets: Vec<DropTargetId>,
    finished: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SurfaceProgramRootDraft {
    root: RootId,
    presentation: SurfaceProgramRootPresentation,
    title_item: Option<ItemId>,
    operations: Vec<SurfaceProgramRootOperation>,
}

impl SurfaceProgram {
    fn link(&mut self, plan: &PresentationPlan) -> Result<(), SceneBuildError> {
        let invalid = || SceneBuildError::InvalidSurfaceProgram {
            surface: plan.surface,
        };
        if !self.finished {
            return Err(invalid());
        }
        let blocks = match std::mem::take(&mut self.tape) {
            SurfaceProgramTape::Draft(blocks) => blocks,
            linked @ SurfaceProgramTape::Linked(_) => {
                self.tape = linked;
                return Ok(());
            }
        };
        let mut instructions = Vec::with_capacity(blocks.len());
        let mut phase = SurfaceProgramPhase::Roots;
        let mut active_root = None;
        let mut contained_count = 0;
        for block in blocks {
            let instruction = match block {
                SurfaceProgramBlock::RootBegin {
                    root,
                    presentation,
                    title_item,
                } => {
                    if active_root.is_some() || phase != SurfaceProgramPhase::Roots {
                        return Err(invalid());
                    }
                    let contained = match presentation {
                        SurfaceProgramRootPresentation::Main => None,
                        SurfaceProgramRootPresentation::Contained(floating) => {
                            plan.contained_records
                                .get(contained_count)
                                .filter(|record| {
                                    record.floating() == floating && record.root() == root
                                })
                                .ok_or_else(invalid)?;
                            let row = ContainedRow(contained_count);
                            contained_count += 1;
                            Some(row)
                        }
                    };
                    let root = LinkedSurfaceProgramRoot {
                        root,
                        contained,
                        title_item,
                    };
                    active_root = Some(root);
                    SurfaceProgramInstruction::RootBegin(root)
                }
                SurfaceProgramBlock::RootOperation(operation) => {
                    let root = active_root.ok_or_else(invalid)?;
                    let operation = plan
                        .link_surface_program_operation(operation, root)
                        .ok_or_else(invalid)?;
                    SurfaceProgramInstruction::RootOperation(operation)
                }
                SurfaceProgramBlock::RootEnd { root } => {
                    active_root
                        .take()
                        .filter(|active| active.root == root)
                        .ok_or_else(invalid)?;
                    SurfaceProgramInstruction::RootEnd(root)
                }
                SurfaceProgramBlock::DragPreview => {
                    if active_root.is_some() || phase != SurfaceProgramPhase::Roots {
                        return Err(invalid());
                    }
                    phase = SurfaceProgramPhase::Targets;
                    SurfaceProgramInstruction::DragPreview
                }
                SurfaceProgramBlock::DropTarget(id) => {
                    if phase != SurfaceProgramPhase::Targets {
                        return Err(invalid());
                    }
                    let target = if plan
                        .surface_background
                        .as_ref()
                        .is_some_and(|record| record.id() == id)
                    {
                        LinkedSurfaceProgramDropTarget::Background
                    } else {
                        let index = plan
                            .drop_targets
                            .binary_search_by_key(&id, |record| record.id())
                            .map_err(|_| invalid())?;
                        LinkedSurfaceProgramDropTarget::Record(DropTargetRow(index))
                    };
                    SurfaceProgramInstruction::DropTarget(target)
                }
                SurfaceProgramBlock::DropGuide(id) => {
                    if !matches!(
                        phase,
                        SurfaceProgramPhase::Targets | SurfaceProgramPhase::Guides
                    ) {
                        return Err(invalid());
                    }
                    phase = SurfaceProgramPhase::Guides;
                    let index = plan
                        .drop_guide_clusters
                        .binary_search_by_key(&id, DropGuideClusterRecord::id)
                        .map_err(|_| invalid())?;
                    SurfaceProgramInstruction::DropGuide(DropGuideRow(index))
                }
                SurfaceProgramBlock::ContainedTransformPreview => {
                    if !matches!(
                        phase,
                        SurfaceProgramPhase::Targets | SurfaceProgramPhase::Guides
                    ) {
                        return Err(invalid());
                    }
                    phase = SurfaceProgramPhase::ContainedPreview;
                    SurfaceProgramInstruction::ContainedTransformPreview
                }
                SurfaceProgramBlock::DragDecoration => {
                    if phase != SurfaceProgramPhase::ContainedPreview {
                        return Err(invalid());
                    }
                    phase = SurfaceProgramPhase::Decoration;
                    SurfaceProgramInstruction::DragDecoration
                }
                SurfaceProgramBlock::PopupBackdrop(session) => {
                    if !matches!(
                        phase,
                        SurfaceProgramPhase::Decoration | SurfaceProgramPhase::PopupBackdrops
                    ) {
                        return Err(invalid());
                    }
                    phase = SurfaceProgramPhase::PopupBackdrops;
                    let index = plan
                        .tab_list_menu_backdrop_records
                        .iter()
                        .position(|record| record.session() == session)
                        .ok_or_else(invalid)?;
                    SurfaceProgramInstruction::PopupBackdrop(PopupBackdropRow(index))
                }
                SurfaceProgramBlock::PopupMenu(session) => {
                    if !matches!(
                        phase,
                        SurfaceProgramPhase::Decoration
                            | SurfaceProgramPhase::PopupBackdrops
                            | SurfaceProgramPhase::PopupMenus
                    ) {
                        return Err(invalid());
                    }
                    phase = SurfaceProgramPhase::PopupMenus;
                    let index = plan
                        .tab_list_menu_records
                        .binary_search_by_key(&session, TabListMenuRecord::session)
                        .map_err(|_| invalid())?;
                    SurfaceProgramInstruction::PopupMenu(PopupMenuRow(index))
                }
                SurfaceProgramBlock::InteractionFeedback => {
                    if !matches!(
                        phase,
                        SurfaceProgramPhase::Decoration
                            | SurfaceProgramPhase::PopupBackdrops
                            | SurfaceProgramPhase::PopupMenus
                    ) {
                        return Err(invalid());
                    }
                    phase = SurfaceProgramPhase::Feedback;
                    SurfaceProgramInstruction::InteractionFeedback
                }
                SurfaceProgramBlock::SurfaceEnd => {
                    if phase != SurfaceProgramPhase::Feedback {
                        return Err(invalid());
                    }
                    phase = SurfaceProgramPhase::End;
                    SurfaceProgramInstruction::SurfaceEnd
                }
            };
            instructions.push(instruction);
        }
        if active_root.is_some()
            || phase != SurfaceProgramPhase::End
            || contained_count != plan.contained_records.len()
        {
            return Err(invalid());
        }
        self.tape = SurfaceProgramTape::Linked(instructions.into_boxed_slice());
        Ok(())
    }

    fn begin_root(
        &mut self,
        root: RootId,
        presentation: SurfaceProgramRootPresentation,
    ) -> Result<(), SurfaceProgramBuildError> {
        if self.finished {
            return Err(SurfaceProgramBuildError::AlreadyFinished);
        }
        if self.active_root.is_some() {
            return Err(SurfaceProgramBuildError::RootAlreadyOpen);
        }
        if self.root_indices.contains_key(&root) {
            return Err(SurfaceProgramBuildError::DuplicateRoot { root });
        }
        let index = self.root_drafts.len();
        self.root_drafts.push(SurfaceProgramRootDraft {
            root,
            presentation,
            title_item: None,
            operations: Vec::new(),
        });
        self.root_indices.insert(root, index);
        self.active_root = Some(index);
        Ok(())
    }

    fn push_root_operation(
        &mut self,
        operation: SurfaceProgramRootOperation,
        title_item: Option<ItemId>,
    ) -> Result<(), SurfaceProgramBuildError> {
        let index = self
            .active_root
            .ok_or(SurfaceProgramBuildError::RootNotOpen)?;
        if self.root_drafts[index].title_item.is_none() {
            self.root_drafts[index].title_item = title_item;
        }
        self.root_drafts[index].operations.push(operation);
        Ok(())
    }

    fn end_root(&mut self, expected: RootId) -> Result<(), SurfaceProgramBuildError> {
        let index = self
            .active_root
            .ok_or(SurfaceProgramBuildError::RootNotOpen)?;
        let actual = self.root_drafts[index].root;
        if actual != expected {
            return Err(SurfaceProgramBuildError::RootMismatch { expected, actual });
        }
        self.active_root = None;
        Ok(())
    }

    fn push_derived_root_operation(
        &mut self,
        root: RootId,
        operation: SurfaceProgramRootOperation,
    ) -> Result<(), SurfaceProgramBuildError> {
        if self.finished {
            return Err(SurfaceProgramBuildError::AlreadyFinished);
        }
        let index = self
            .root_indices
            .get(&root)
            .copied()
            .ok_or(SurfaceProgramBuildError::MissingRoot { root })?;
        self.root_drafts[index].operations.push(operation);
        Ok(())
    }

    pub(super) fn defer_popup_backdrop(&mut self, session: TabListMenuSessionId) {
        self.deferred_popup_backdrops.push(session);
    }

    pub(super) fn defer_popup_menu(&mut self, session: TabListMenuSessionId) {
        self.deferred_popup_menus.push(session);
    }

    pub(super) fn defer_drop_target(&mut self, id: DropTargetId) {
        self.deferred_drop_targets.push(id);
    }

    pub(super) fn defer_surface_background(&mut self, id: DropTargetId) {
        self.deferred_surface_background = Some(id);
    }

    pub(super) fn defer_guide(&mut self, cluster: &DropGuideClusterRecord) {
        self.deferred_guides.push(cluster.id());
        self.deferred_guide_targets
            .extend(cluster.targets().map(|(_, target)| target.id()));
    }

    pub(super) fn finish(&mut self) -> Result<(), SurfaceProgramBuildError> {
        if self.finished {
            return Err(SurfaceProgramBuildError::AlreadyFinished);
        }
        if self.active_root.is_some() {
            return Err(SurfaceProgramBuildError::RootAlreadyOpen);
        }
        {
            let mut seen_guides =
                std::collections::HashSet::with_capacity(self.deferred_guides.len());
            for id in &self.deferred_guides {
                if !seen_guides.insert(id) {
                    return Err(SurfaceProgramBuildError::DuplicateGuide { id: *id });
                }
            }
        }
        {
            let mut seen_popup_backdrops =
                std::collections::HashSet::with_capacity(self.deferred_popup_backdrops.len());
            for session in &self.deferred_popup_backdrops {
                if !seen_popup_backdrops.insert(session) {
                    return Err(SurfaceProgramBuildError::DuplicatePopupBackdrop {
                        session: *session,
                    });
                }
            }
            {
                let mut seen_popup_menus =
                    std::collections::HashSet::with_capacity(self.deferred_popup_menus.len());
                for session in &self.deferred_popup_menus {
                    if !seen_popup_menus.insert(session) {
                        return Err(SurfaceProgramBuildError::DuplicatePopupMenu {
                            session: *session,
                        });
                    }
                }
            }
            for session in &self.deferred_popup_menus {
                if !seen_popup_backdrops.contains(session) {
                    return Err(SurfaceProgramBuildError::PopupMenuWithoutBackdrop {
                        session: *session,
                    });
                }
            }
        }
        // A non-owner surface may legitimately retain only the workspace
        // backdrop while another surface owns the popup menu.
        {
            let mut seen_drop_targets =
                std::collections::HashSet::with_capacity(self.deferred_drop_targets.len());
            for id in &self.deferred_drop_targets {
                if !seen_drop_targets.insert(id) {
                    return Err(SurfaceProgramBuildError::DuplicateDropTarget { id: *id });
                }
            }
        }
        for root in &self.root_drafts {
            let mut seen_operations =
                std::collections::HashSet::with_capacity(root.operations.len());
            for operation in &root.operations {
                if !seen_operations.insert(operation) {
                    return Err(SurfaceProgramBuildError::DuplicateOperation {
                        root: root.root,
                        operation: *operation,
                    });
                }
            }
        }
        let mut blocks = Vec::new();
        let root_drafts = std::mem::take(&mut self.root_drafts);
        self.root_indices.clear();
        for mut root in root_drafts {
            root.operations
                .sort_by_key(|operation| operation.paint_rank());
            blocks.push(SurfaceProgramBlock::RootBegin {
                root: root.root,
                presentation: root.presentation,
                title_item: root.title_item,
            });
            blocks.extend(
                root.operations
                    .iter()
                    .copied()
                    .map(SurfaceProgramBlock::RootOperation),
            );
            blocks.push(SurfaceProgramBlock::RootEnd { root: root.root });
        }
        blocks.push(SurfaceProgramBlock::DragPreview);
        if let Some(id) = self.deferred_surface_background.take() {
            blocks.push(SurfaceProgramBlock::DropTarget(id));
        }
        let deferred_guide_targets = std::mem::take(&mut self.deferred_guide_targets);
        let mut guide_targets =
            std::collections::HashSet::<&DropTargetId>::with_capacity(deferred_guide_targets.len());
        guide_targets.extend(deferred_guide_targets.iter());
        blocks.extend(
            std::mem::take(&mut self.deferred_drop_targets)
                .into_iter()
                .filter(|id| id.kind() == DropTargetKind::TabGap && !guide_targets.contains(id))
                .map(SurfaceProgramBlock::DropTarget),
        );
        for id in std::mem::take(&mut self.deferred_guides) {
            blocks.push(SurfaceProgramBlock::DropGuide(id));
        }
        blocks.push(SurfaceProgramBlock::ContainedTransformPreview);
        blocks.push(SurfaceProgramBlock::DragDecoration);
        blocks.extend(
            std::mem::take(&mut self.deferred_popup_backdrops)
                .into_iter()
                .map(SurfaceProgramBlock::PopupBackdrop),
        );
        blocks.extend(
            std::mem::take(&mut self.deferred_popup_menus)
                .into_iter()
                .map(SurfaceProgramBlock::PopupMenu),
        );
        blocks.push(SurfaceProgramBlock::InteractionFeedback);
        blocks.push(SurfaceProgramBlock::SurfaceEnd);
        self.tape = SurfaceProgramTape::Draft(blocks);
        self.finished = true;
        Ok(())
    }
}

impl SurfaceProgramRootOperation {
    const fn paint_rank(self) -> usize {
        match self {
            Self::Pane(_) => 0,
            Self::TabBar(_) => 1,
            Self::Tab(_) => 2,
            Self::Splitter(_) | Self::SplitterJunction(_) => 3,
            Self::TabStripControl(_) => 4,
            Self::ContainedControls(_) => 5,
            Self::PresentationMenuAnchor(_) => 6,
        }
    }
}

impl PresentationPlan {
    pub(crate) fn begin_surface_program_root(
        &mut self,
        root: RootId,
        presentation: SurfaceProgramRootPresentation,
    ) -> Result<(), SurfaceProgramBuildError> {
        self.surface_program.begin_root(root, presentation)
    }

    pub(crate) fn push_surface_program_root_operation(
        &mut self,
        operation: SurfaceProgramRootOperation,
    ) -> Result<(), SurfaceProgramBuildError> {
        self.surface_program.push_root_operation(operation, None)
    }

    pub(crate) fn push_surface_program_pane_operation(
        &mut self,
        id: PaneSceneId,
        selected: Option<ItemId>,
    ) -> Result<(), SurfaceProgramBuildError> {
        self.surface_program
            .push_root_operation(SurfaceProgramRootOperation::Pane(id), selected)
    }

    pub(crate) fn push_surface_program_derived_root_operation(
        &mut self,
        root: RootId,
        operation: SurfaceProgramRootOperation,
    ) -> Result<(), SurfaceProgramBuildError> {
        self.surface_program
            .push_derived_root_operation(root, operation)
    }

    pub(crate) fn end_surface_program_root(
        &mut self,
        root: RootId,
    ) -> Result<(), SurfaceProgramBuildError> {
        self.surface_program.end_root(root)
    }

    pub(crate) fn finish_surface_program(&mut self) -> Result<(), SurfaceProgramBuildError> {
        self.surface_program.finish()
    }

    /// Consumes the construction identities after canonical record ordering is final.
    pub(crate) fn link_surface_program(&mut self) -> Result<(), SceneBuildError> {
        if matches!(self.surface_program.tape, SurfaceProgramTape::Linked(_)) {
            return Ok(());
        }
        let mut program = std::mem::take(&mut self.surface_program);
        program.link(self)?;
        self.surface_program = program;
        Ok(())
    }

    fn link_surface_program_operation(
        &self,
        operation: SurfaceProgramRootOperation,
        root: LinkedSurfaceProgramRoot,
    ) -> Option<LinkedSurfaceProgramOperation> {
        Some(match operation {
            SurfaceProgramRootOperation::Pane(id) if id.root == root.root => {
                LinkedSurfaceProgramOperation::Pane(PaneRow(
                    self.pane_records
                        .binary_search_by_key(&id, PaneRecord::id)
                        .ok()?,
                ))
            }
            SurfaceProgramRootOperation::TabBar(id) if id.root == root.root => {
                LinkedSurfaceProgramOperation::TabBar(TabBarRow(
                    self.tab_bar_records
                        .binary_search_by_key(&id, |record| *record.id())
                        .ok()?,
                ))
            }
            SurfaceProgramRootOperation::Tab(id) if id.root == root.root => {
                LinkedSurfaceProgramOperation::Tab(TabRow(
                    self.tab_records
                        .binary_search_by_key(&id, |record| *record.id())
                        .ok()?,
                ))
            }
            SurfaceProgramRootOperation::Splitter(id) if id.root == root.root => {
                LinkedSurfaceProgramOperation::Splitter(SplitterRow(
                    self.splitter_records
                        .binary_search_by_key(&id, |record| *record.id())
                        .ok()?,
                ))
            }
            SurfaceProgramRootOperation::SplitterJunction(id) if id.root() == root.root => {
                LinkedSurfaceProgramOperation::SplitterJunction(SplitterJunctionRow(
                    self.splitter_junction_records
                        .binary_search_by_key(&id, SplitterJunctionRecord::id)
                        .ok()?,
                ))
            }
            SurfaceProgramRootOperation::TabStripControl(id) if id.bar().root == root.root => {
                LinkedSurfaceProgramOperation::TabStripControl(TabStripControlRow(
                    self.tab_strip_control_records
                        .binary_search_by_key(&id, |record| record.id())
                        .ok()?,
                ))
            }
            SurfaceProgramRootOperation::ContainedControls(floating) => {
                let row = root.contained?;
                if self.contained_records.get(row.0)?.floating() != floating {
                    return None;
                }
                LinkedSurfaceProgramOperation::ContainedControls(row)
            }
            SurfaceProgramRootOperation::PresentationMenuAnchor(expected)
                if expected == root.root =>
            {
                LinkedSurfaceProgramOperation::PresentationMenuAnchor(PresentationMenuAnchorRow(
                    self.presentation_menu_anchor_records
                        .binary_search_by_key(&expected, |record| record.root())
                        .ok()?,
                ))
            }
            _ => return None,
        })
    }

    pub(crate) fn visit_surface_program<'plan, E>(
        &'plan self,
        mut visitor: impl FnMut(ResolvedSurfaceProgramStep<'plan>) -> Result<(), E>,
    ) -> Result<(), SurfaceProgramVisitError<E>> {
        let SurfaceProgramTape::Linked(instructions) = &self.surface_program.tape else {
            return Err(SurfaceProgramVisitError::InvalidProgram);
        };
        let invalid = || SurfaceProgramVisitError::InvalidProgram;
        let mut active_root = None;
        for instruction in instructions.iter() {
            let step = match *instruction {
                SurfaceProgramInstruction::RootBegin(root) => {
                    let contained = root
                        .contained
                        .map(|row| self.contained_records.get(row.0).ok_or_else(invalid))
                        .transpose()?;
                    let root = ResolvedSurfaceProgramRoot {
                        root: root.root,
                        contained,
                        title_item: root.title_item,
                    };
                    active_root = Some(root);
                    ResolvedSurfaceProgramStep::RootBegin(root)
                }
                SurfaceProgramInstruction::RootOperation(operation) => {
                    let root = active_root.ok_or_else(invalid)?;
                    let operation = self
                        .resolve_linked_surface_program_operation(operation)
                        .ok_or_else(invalid)?;
                    ResolvedSurfaceProgramStep::Operation { root, operation }
                }
                SurfaceProgramInstruction::RootEnd(expected) => {
                    let root = active_root
                        .take()
                        .filter(|root| root.root() == expected)
                        .ok_or_else(invalid)?;
                    ResolvedSurfaceProgramStep::RootEnd(root)
                }
                SurfaceProgramInstruction::PopupBackdrop(row) => {
                    ResolvedSurfaceProgramStep::PopupBackdrop(
                        self.tab_list_menu_backdrop_records
                            .get(row.0)
                            .ok_or_else(invalid)?,
                    )
                }
                SurfaceProgramInstruction::PopupMenu(row) => ResolvedSurfaceProgramStep::PopupMenu(
                    self.tab_list_menu_records.get(row.0).ok_or_else(invalid)?,
                ),
                SurfaceProgramInstruction::DragPreview => ResolvedSurfaceProgramStep::DragPreview,
                SurfaceProgramInstruction::DropTarget(target) => {
                    let record = match target {
                        LinkedSurfaceProgramDropTarget::Background => {
                            self.surface_background.as_ref()
                        }
                        LinkedSurfaceProgramDropTarget::Record(row) => {
                            self.drop_targets.get(row.0).map(AsRef::as_ref)
                        }
                    }
                    .ok_or_else(invalid)?;
                    ResolvedSurfaceProgramStep::DropTarget(record)
                }
                SurfaceProgramInstruction::DropGuide(row) => ResolvedSurfaceProgramStep::DropGuide(
                    self.drop_guide_clusters.get(row.0).ok_or_else(invalid)?,
                ),
                SurfaceProgramInstruction::ContainedTransformPreview => {
                    ResolvedSurfaceProgramStep::ContainedTransformPreview
                }
                SurfaceProgramInstruction::DragDecoration => {
                    ResolvedSurfaceProgramStep::DragDecoration
                }
                SurfaceProgramInstruction::InteractionFeedback => {
                    ResolvedSurfaceProgramStep::InteractionFeedback
                }
                SurfaceProgramInstruction::SurfaceEnd => ResolvedSurfaceProgramStep::SurfaceEnd,
            };
            visitor(step).map_err(SurfaceProgramVisitError::Sink)?;
        }
        Ok(())
    }

    fn resolve_linked_surface_program_operation(
        &self,
        operation: LinkedSurfaceProgramOperation,
    ) -> Option<ResolvedSurfaceProgramOperation<'_>> {
        Some(match operation {
            LinkedSurfaceProgramOperation::Pane(row) => {
                ResolvedSurfaceProgramOperation::Pane(self.pane_records.get(row.0)?)
            }
            LinkedSurfaceProgramOperation::TabBar(row) => {
                ResolvedSurfaceProgramOperation::TabBar(self.tab_bar_records.get(row.0)?)
            }
            LinkedSurfaceProgramOperation::Tab(row) => {
                ResolvedSurfaceProgramOperation::Tab(self.tab_records.get(row.0)?)
            }
            LinkedSurfaceProgramOperation::Splitter(row) => {
                ResolvedSurfaceProgramOperation::Splitter(self.splitter_records.get(row.0)?)
            }
            LinkedSurfaceProgramOperation::SplitterJunction(row) => {
                ResolvedSurfaceProgramOperation::SplitterJunction(
                    self.splitter_junction_records.get(row.0)?,
                )
            }
            LinkedSurfaceProgramOperation::TabStripControl(row) => {
                ResolvedSurfaceProgramOperation::TabStripControl(
                    self.tab_strip_control_records.get(row.0)?,
                )
            }
            LinkedSurfaceProgramOperation::ContainedControls(row) => {
                ResolvedSurfaceProgramOperation::ContainedControls(
                    self.contained_records.get(row.0)?,
                )
            }
            LinkedSurfaceProgramOperation::PresentationMenuAnchor(row) => {
                ResolvedSurfaceProgramOperation::PresentationMenuAnchor(
                    self.presentation_menu_anchor_records.get(row.0)?,
                )
            }
        })
    }

    pub(crate) fn surface_program_menu_open_bars(
        &self,
    ) -> impl Iterator<Item = TabBarSceneId> + '_ {
        let instructions = match &self.surface_program.tape {
            SurfaceProgramTape::Linked(instructions) => instructions.as_ref(),
            SurfaceProgramTape::Draft(_) => &[],
        };
        instructions
            .iter()
            .filter_map(|instruction| match instruction {
                SurfaceProgramInstruction::PopupMenu(row) => self
                    .tab_list_menu_records
                    .get(row.0)
                    .map(TabListMenuRecord::bar),
                _ => None,
            })
    }

    #[cfg(test)]
    pub(crate) const fn surface_program(&self) -> SurfaceProgramInspection<'_> {
        SurfaceProgramInspection { plan: self }
    }

    #[cfg(test)]
    fn remove_surface_program_block_for_test(
        &mut self,
        predicate: impl FnMut(&SurfaceProgramBlock) -> bool,
    ) -> bool {
        let Some(index) = self.surface_program().blocks().iter().position(predicate) else {
            return false;
        };
        match &mut self.surface_program.tape {
            SurfaceProgramTape::Draft(blocks) => {
                blocks.remove(index);
            }
            SurfaceProgramTape::Linked(instructions) => {
                let mut remaining = std::mem::take(instructions).into_vec();
                remaining.remove(index);
                *instructions = remaining.into_boxed_slice();
            }
        }
        true
    }

    #[cfg(test)]
    pub(crate) fn remove_surface_program_operation_for_test(
        &mut self,
        operation: SurfaceProgramRootOperation,
    ) -> bool {
        self.remove_surface_program_block_for_test(|block| {
            matches!(block, SurfaceProgramBlock::RootOperation(current) if *current == operation)
        })
    }

    #[cfg(test)]
    pub(crate) fn remove_surface_program_popup_menu_for_test(
        &mut self,
        session: TabListMenuSessionId,
    ) -> bool {
        self.remove_surface_program_block_for_test(
            |block| matches!(block, SurfaceProgramBlock::PopupMenu(current) if *current == session),
        )
    }

    #[cfg(test)]
    pub(crate) fn remove_surface_program_drop_target_for_test(
        &mut self,
        target: DropTargetId,
    ) -> bool {
        self.remove_surface_program_block_for_test(
            |block| matches!(block, SurfaceProgramBlock::DropTarget(current) if *current == target),
        )
    }
}

/// Test inspection reconstructs semantic identities from the actual executable tape.
#[cfg(test)]
pub(crate) struct SurfaceProgramInspection<'a> {
    plan: &'a PresentationPlan,
}

#[cfg(test)]
impl SurfaceProgramInspection<'_> {
    pub(crate) fn blocks(&self) -> Vec<SurfaceProgramBlock> {
        let instructions = match &self.plan.surface_program.tape {
            SurfaceProgramTape::Draft(blocks) => return blocks.clone(),
            SurfaceProgramTape::Linked(instructions) => instructions,
        };
        instructions
            .iter()
            .map(|instruction| match *instruction {
                SurfaceProgramInstruction::RootBegin(root) => SurfaceProgramBlock::RootBegin {
                    root: root.root,
                    presentation: root.contained.map_or(
                        SurfaceProgramRootPresentation::Main,
                        |row| {
                            SurfaceProgramRootPresentation::Contained(
                                self.plan.contained_records[row.0].floating(),
                            )
                        },
                    ),
                    title_item: root.title_item,
                },
                SurfaceProgramInstruction::RootOperation(operation) => {
                    let operation = self
                        .plan
                        .resolve_linked_surface_program_operation(operation)
                        .expect("linked test inspection must resolve the actual record row");
                    SurfaceProgramBlock::RootOperation(match operation {
                        ResolvedSurfaceProgramOperation::Pane(record) => {
                            SurfaceProgramRootOperation::Pane(record.id())
                        }
                        ResolvedSurfaceProgramOperation::TabBar(record) => {
                            SurfaceProgramRootOperation::TabBar(*record.id())
                        }
                        ResolvedSurfaceProgramOperation::Tab(record) => {
                            SurfaceProgramRootOperation::Tab(*record.id())
                        }
                        ResolvedSurfaceProgramOperation::Splitter(record) => {
                            SurfaceProgramRootOperation::Splitter(*record.id())
                        }
                        ResolvedSurfaceProgramOperation::SplitterJunction(record) => {
                            SurfaceProgramRootOperation::SplitterJunction(record.id())
                        }
                        ResolvedSurfaceProgramOperation::TabStripControl(record) => {
                            SurfaceProgramRootOperation::TabStripControl(record.id())
                        }
                        ResolvedSurfaceProgramOperation::ContainedControls(record) => {
                            SurfaceProgramRootOperation::ContainedControls(record.floating())
                        }
                        ResolvedSurfaceProgramOperation::PresentationMenuAnchor(record) => {
                            SurfaceProgramRootOperation::PresentationMenuAnchor(record.root())
                        }
                    })
                }
                SurfaceProgramInstruction::RootEnd(root) => SurfaceProgramBlock::RootEnd { root },
                SurfaceProgramInstruction::PopupBackdrop(row) => {
                    SurfaceProgramBlock::PopupBackdrop(
                        self.plan.tab_list_menu_backdrop_records[row.0].session(),
                    )
                }
                SurfaceProgramInstruction::PopupMenu(row) => {
                    SurfaceProgramBlock::PopupMenu(self.plan.tab_list_menu_records[row.0].session())
                }
                SurfaceProgramInstruction::DragPreview => SurfaceProgramBlock::DragPreview,
                SurfaceProgramInstruction::DropTarget(target) => {
                    SurfaceProgramBlock::DropTarget(match target {
                        LinkedSurfaceProgramDropTarget::Background => self
                            .plan
                            .surface_background
                            .as_ref()
                            .expect("linked test inspection must retain its background record")
                            .id(),
                        LinkedSurfaceProgramDropTarget::Record(row) => {
                            self.plan.drop_targets[row.0].id()
                        }
                    })
                }
                SurfaceProgramInstruction::DropGuide(row) => {
                    SurfaceProgramBlock::DropGuide(self.plan.drop_guide_clusters[row.0].id())
                }
                SurfaceProgramInstruction::ContainedTransformPreview => {
                    SurfaceProgramBlock::ContainedTransformPreview
                }
                SurfaceProgramInstruction::DragDecoration => SurfaceProgramBlock::DragDecoration,
                SurfaceProgramInstruction::InteractionFeedback => {
                    SurfaceProgramBlock::InteractionFeedback
                }
                SurfaceProgramInstruction::SurfaceEnd => SurfaceProgramBlock::SurfaceEnd,
            })
            .collect()
    }
}
