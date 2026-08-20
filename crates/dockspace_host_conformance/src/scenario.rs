//! Private-workspace semantic scenarios shared by conformance executors.
//!
//! These values name product concepts only. Each executor privately maps them
//! to its own public framework facts; the scenario never carries graph IDs,
//! geometry, hit-test results, or presentation receipts.

/// Stable identifier for one semantic product scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioId {
    /// Exercise local tab, close, splitter, and contained semantics.
    LocalTabCloseSplitterAndContained,
    /// Exercise one managed native create, first-live, redock, and retirement lifecycle.
    NativeCreateRedockRetirement,
}

/// Scenario-local item name, independent of any runtime item identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemAlias {
    /// The fixture's initially selected item.
    Primary,
    /// The fixture's second item.
    Secondary,
    /// The item presented by the fixture's contained root.
    Contained,
}

/// Scenario-local layout axis, independent of runtime graph identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioAxis {
    /// Left-to-right split adjustment.
    Horizontal,
    /// Top-to-bottom split adjustment.
    Vertical,
}

/// Scenario-local contained edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioEdge {
    /// The contained presentation's right edge.
    East,
}

/// Scenario-local surface name, independent of runtime surface identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceAlias {
    /// The fixture's original recovery surface.
    Source,
    /// The managed child created by the scenario.
    Child,
}

/// Product action named by a lifecycle observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioProductAction {
    /// Move one root into a newly created native child.
    MoveToNewWindow,
    /// Return the native child root to its recovery surface.
    DockBack,
}

/// Terminal status of one product action request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioActionTerminality {
    /// The core accepted the request and began its presentation transition.
    Requested,
    /// The core rejected the request without beginning a transition.
    Rejected,
}

/// Semantic product control selected by one scenario step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticSelector {
    /// The tab body representing an item.
    Tab(ItemAlias),
    /// The close control representing an item.
    TabClose(ItemAlias),
    /// The splitter moving panes along one semantic axis.
    Splitter(ScenarioAxis),
    /// One cardinal resize control of a contained presentation.
    ContainedResize {
        /// Item identifying the contained presentation without exposing its runtime identity.
        item: ItemAlias,
        /// Cardinal edge selected by the scenario.
        edge: ScenarioEdge,
    },
}

/// Framework-neutral action applied to a semantic selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticAction {
    /// Activate the selected control through the executor's semantic path.
    Activate,
    /// Increase the selected splitter or contained edge by one semantic step.
    Increment,
}

/// Product observation reported after a scenario step settles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticObservation {
    /// The named item became selected.
    ItemSelected(ItemAlias),
    /// The product emitted a close request for the named item.
    CloseRequested(ItemAlias),
    /// The selected splitter advanced along its semantic axis.
    SplitterAdjusted(ScenarioAxis),
    /// The selected contained edge advanced while the opposite edge stayed fixed.
    ContainedResized {
        /// Item identifying the resized contained presentation.
        item: ItemAlias,
        /// Cardinal edge advanced by the action.
        edge: ScenarioEdge,
    },
    /// One product request reached an explicit terminal status.
    ActionTerminal {
        /// Product action whose request settled.
        action: ScenarioProductAction,
        /// Exact request status reported by the executor.
        terminality: ScenarioActionTerminality,
    },
    /// Product ownership was published on the named surface.
    PresentationApplied(SurfaceAlias),
    /// The named surface crossed its exact first-live presentation barrier.
    FirstLive(SurfaceAlias),
    /// The named surface binding was retired after its presentation moved away.
    Retired(SurfaceAlias),
    /// Every binding-scoped callback, effect, output, and presentation reference was reclaimed.
    Quiescent,
}

/// Capability required to execute a semantic scenario without guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioCapability {
    /// The executor can activate local tab semantics.
    LocalTabSelection,
    /// The executor can activate and observe local close semantics.
    LocalItemClose,
    /// The executor can adjust an exact local splitter semantically.
    LocalSplitterAdjustment,
    /// The executor can adjust an exact contained resize edge semantically.
    LocalContainedResize,
    /// The executor can drive typed native-window create and close effects.
    NativeWindowLifecycle,
    /// The executor can observe exact first-live admission.
    NativeFirstLive,
    /// The executor can prove retirement of an exact child binding.
    NativeRetirement,
    /// The executor can publish and observe permanent binding quiescence.
    NativeQuiescence,
}

/// Executor decision made before any scenario input is scheduled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioAdmission {
    /// Every declared capability is available to the executor.
    Ready,
    /// The executor must skip or reject the scenario without guessing.
    Unsupported {
        /// First missing capability in the scenario's declared order.
        missing: ScenarioCapability,
    },
}

/// One selector/action pair and its required product observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticStep {
    selector: SemanticSelector,
    action: SemanticAction,
    expected: SemanticObservation,
}

impl SemanticStep {
    const fn new(
        selector: SemanticSelector,
        action: SemanticAction,
        expected: SemanticObservation,
    ) -> Self {
        Self {
            selector,
            action,
            expected,
        }
    }

    /// Returns the product control selected by this step.
    #[must_use]
    pub const fn selector(self) -> SemanticSelector {
        self.selector
    }

    /// Returns the framework-neutral action applied by this step.
    #[must_use]
    pub const fn action(self) -> SemanticAction {
        self.action
    }

    /// Returns the product observation required after settlement.
    #[must_use]
    pub const fn expected(self) -> SemanticObservation {
        self.expected
    }
}

/// Immutable semantic scenario description consumed by private executors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticScenario {
    id: ScenarioId,
    required_capabilities: &'static [ScenarioCapability],
    steps: &'static [SemanticStep],
}

/// Immutable lifecycle scenario consumed only by executors with native facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticLifecycleScenario {
    id: ScenarioId,
    required_capabilities: &'static [ScenarioCapability],
    expected_observations: &'static [SemanticObservation],
}

impl SemanticLifecycleScenario {
    const fn new(
        id: ScenarioId,
        required_capabilities: &'static [ScenarioCapability],
        expected_observations: &'static [SemanticObservation],
    ) -> Self {
        Self {
            id,
            required_capabilities,
            expected_observations,
        }
    }

    /// Returns the stable semantic scenario identifier.
    #[must_use]
    pub const fn id(self) -> ScenarioId {
        self.id
    }

    /// Returns native capabilities which must be proven before execution.
    #[must_use]
    pub const fn required_capabilities(self) -> &'static [ScenarioCapability] {
        self.required_capabilities
    }

    /// Decides whether an executor may schedule this scenario.
    #[must_use]
    pub fn admission(self, supported: &[ScenarioCapability]) -> ScenarioAdmission {
        scenario_admission(self.required_capabilities, supported)
    }

    /// Returns lifecycle observations in their required causal order.
    #[must_use]
    pub const fn expected_observations(self) -> &'static [SemanticObservation] {
        self.expected_observations
    }
}

impl SemanticScenario {
    const fn new(
        id: ScenarioId,
        required_capabilities: &'static [ScenarioCapability],
        steps: &'static [SemanticStep],
    ) -> Self {
        Self {
            id,
            required_capabilities,
            steps,
        }
    }

    /// Returns the stable semantic scenario identifier.
    #[must_use]
    pub const fn id(self) -> ScenarioId {
        self.id
    }

    /// Returns capabilities that an executor must prove before running.
    #[must_use]
    pub const fn required_capabilities(self) -> &'static [ScenarioCapability] {
        self.required_capabilities
    }

    /// Decides whether an executor may schedule this scenario.
    #[must_use]
    pub fn admission(self, supported: &[ScenarioCapability]) -> ScenarioAdmission {
        scenario_admission(self.required_capabilities, supported)
    }

    /// Returns the ordered semantic steps.
    #[must_use]
    pub const fn steps(self) -> &'static [SemanticStep] {
        self.steps
    }

    /// Collects the ordered observations required by the scenario.
    #[must_use]
    pub fn expected_observations(self) -> Vec<SemanticObservation> {
        self.steps.iter().map(|step| step.expected()).collect()
    }
}

fn scenario_admission(
    required: &[ScenarioCapability],
    supported: &[ScenarioCapability],
) -> ScenarioAdmission {
    required
        .iter()
        .copied()
        .find(|capability| !supported.contains(capability))
        .map_or(ScenarioAdmission::Ready, |missing| {
            ScenarioAdmission::Unsupported { missing }
        })
}

const LOCAL_INTERACTION_CAPABILITIES: &[ScenarioCapability] = &[
    ScenarioCapability::LocalTabSelection,
    ScenarioCapability::LocalItemClose,
    ScenarioCapability::LocalSplitterAdjustment,
    ScenarioCapability::LocalContainedResize,
];
const LOCAL_INTERACTION_STEPS: &[SemanticStep] = &[
    SemanticStep::new(
        SemanticSelector::Tab(ItemAlias::Secondary),
        SemanticAction::Activate,
        SemanticObservation::ItemSelected(ItemAlias::Secondary),
    ),
    SemanticStep::new(
        SemanticSelector::TabClose(ItemAlias::Secondary),
        SemanticAction::Activate,
        SemanticObservation::CloseRequested(ItemAlias::Secondary),
    ),
    SemanticStep::new(
        SemanticSelector::Splitter(ScenarioAxis::Horizontal),
        SemanticAction::Increment,
        SemanticObservation::SplitterAdjusted(ScenarioAxis::Horizontal),
    ),
    SemanticStep::new(
        SemanticSelector::ContainedResize {
            item: ItemAlias::Contained,
            edge: ScenarioEdge::East,
        },
        SemanticAction::Increment,
        SemanticObservation::ContainedResized {
            item: ItemAlias::Contained,
            edge: ScenarioEdge::East,
        },
    ),
];

/// Shared local interaction scenario executed by both conformance adapters.
pub const LOCAL_TAB_CLOSE_SPLITTER_AND_CONTAINED: SemanticScenario = SemanticScenario::new(
    ScenarioId::LocalTabCloseSplitterAndContained,
    LOCAL_INTERACTION_CAPABILITIES,
    LOCAL_INTERACTION_STEPS,
);

const NATIVE_LIFECYCLE_CAPABILITIES: &[ScenarioCapability] = &[
    ScenarioCapability::NativeWindowLifecycle,
    ScenarioCapability::NativeFirstLive,
    ScenarioCapability::NativeRetirement,
    ScenarioCapability::NativeQuiescence,
];
const NATIVE_LIFECYCLE_OBSERVATIONS: &[SemanticObservation] = &[
    SemanticObservation::ActionTerminal {
        action: ScenarioProductAction::MoveToNewWindow,
        terminality: ScenarioActionTerminality::Requested,
    },
    SemanticObservation::PresentationApplied(SurfaceAlias::Child),
    SemanticObservation::FirstLive(SurfaceAlias::Child),
    SemanticObservation::ActionTerminal {
        action: ScenarioProductAction::DockBack,
        terminality: ScenarioActionTerminality::Requested,
    },
    SemanticObservation::PresentationApplied(SurfaceAlias::Source),
    SemanticObservation::Retired(SurfaceAlias::Child),
    SemanticObservation::Quiescent,
];

/// Managed lifecycle scenario rejected by executors without native authority.
pub const NATIVE_CREATE_REDOCK_RETIREMENT: SemanticLifecycleScenario =
    SemanticLifecycleScenario::new(
        ScenarioId::NativeCreateRedockRetirement,
        NATIVE_LIFECYCLE_CAPABILITIES,
        NATIVE_LIFECYCLE_OBSERVATIONS,
    );
