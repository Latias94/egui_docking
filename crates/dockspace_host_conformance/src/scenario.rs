//! Private-workspace semantic scenarios shared by conformance executors.
//!
//! These values name product concepts only. Each executor privately maps them
//! to its own public framework facts; the scenario never carries graph IDs,
//! geometry, hit-test results, or presentation receipts.

/// Stable identifier for one U1 behavior-spine journey.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum JourneyId {
    /// Tab selection and drag-threshold ownership.
    L01,
    /// Local tab reorder.
    L02,
    /// Center and edge docking basins.
    L03,
    /// Contained move, resize, and re-dock.
    L04,
    /// Cancellation, stale authority, and target loss.
    L05,
    /// Presentation-qualified release.
    L06,
    /// Group, title, menu, and close ownership.
    L07,
    /// Layout-command and input-path convergence.
    L08,
    /// Programmatic native lifecycle.
    N01,
    /// Physical native drag-out and re-dock.
    P01,
}

/// Input cadence named without prescribing frames or coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CadenceProfile {
    /// Press, motion, and release arrive in one bounded burst.
    Burst,
    /// Meaningful gesture transitions arrive separately.
    Stepwise,
    /// Motion hesitates around an acquired target.
    Hesitant,
    /// The final move and release share one batch.
    ReleaseWithFinalMove,
    /// Candidate presentation is discarded before release.
    DiscardBeforeRelease,
    /// The current target disappears during manipulation.
    TargetDisappears,
    /// Touch arms through a long press before manipulation.
    TouchLongPress,
}

/// Fixed journey entry describing its input cadences and required capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JourneySpec {
    id: JourneyId,
    title: &'static str,
    cadences: &'static [CadenceProfile],
    required_capabilities: &'static [ScenarioCapability],
}

impl JourneySpec {
    const fn new(
        id: JourneyId,
        title: &'static str,
        cadences: &'static [CadenceProfile],
        required_capabilities: &'static [ScenarioCapability],
    ) -> Self {
        Self {
            id,
            title,
            cadences,
            required_capabilities,
        }
    }

    /// Returns the stable journey identifier.
    #[must_use]
    pub const fn id(self) -> JourneyId {
        self.id
    }

    /// Returns the short journey title.
    pub const fn title(self) -> &'static str {
        self.title
    }

    /// Returns the assigned input cadences.
    pub const fn cadences(self) -> &'static [CadenceProfile] {
        self.cadences
    }

    /// Returns facts which an executor must prove before scheduling input.
    pub const fn required_capabilities(self) -> &'static [ScenarioCapability] {
        self.required_capabilities
    }

    /// Decides whether an executor may schedule this journey.
    #[must_use]
    pub fn admission(self, supported: &[ScenarioCapability]) -> ScenarioAdmission {
        scenario_admission(self.required_capabilities, supported)
    }
}

/// Stable identifier for one semantic product scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioId {
    /// Exercise center and cardinal magnetic docking basins.
    MagneticDockingBasins,
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

/// Core-owned magnetic basin selected by an ordinary pane-content release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MagneticDockBasin {
    /// Merge into the target tab stack.
    Center,
    /// Split above the target.
    North,
    /// Split below the target.
    South,
    /// Split to the target's right.
    East,
    /// Split to the target's left.
    West,
}

/// Strictly inset point normalized to the executor's current target pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormalizedInsetSample {
    x_per_mille: u16,
    y_per_mille: u16,
}

impl NormalizedInsetSample {
    const fn new(x_per_mille: u16, y_per_mille: u16) -> Self {
        assert!(x_per_mille > 0 && x_per_mille < 1_000);
        assert!(y_per_mille > 0 && y_per_mille < 1_000);
        Self {
            x_per_mille,
            y_per_mille,
        }
    }

    /// Returns the horizontal pane fraction in the open interval `(0, 1)`.
    #[must_use]
    pub fn x(self) -> f64 {
        f64::from(self.x_per_mille) / 1_000.0
    }

    /// Returns the vertical pane fraction in the open interval `(0, 1)`.
    #[must_use]
    pub fn y(self) -> f64 {
        f64::from(self.y_per_mille) / 1_000.0
    }
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
    /// The named item committed through one ordinary magnetic basin.
    ItemDocked {
        /// Item moved by the shared journey.
        item: ItemAlias,
        /// Core-owned target basin selected by the preview and release.
        basin: MagneticDockBasin,
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
    /// The executor can drive ordinary local direct manipulation.
    LocalDirectManipulation,
    /// The executor can acquire semantic docking basins without guide-center targeting.
    SemanticDockBasins,
    /// The executor can observe the exact candidate used by release.
    PresentedCandidate,
    /// The executor can observe a complete contained-window projection.
    FullContainedProjection,
    /// The executor can arm touch manipulation through a long press.
    TouchLongPress,
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
    /// The executor has the complete physical native evidence lane.
    NativePhysicalAuthority,
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
    journey: JourneyId,
    required_capabilities: &'static [ScenarioCapability],
    steps: &'static [SemanticStep],
}

/// One finite L03 input case shared by the core-host and product executors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MagneticDockCase {
    source: ItemAlias,
    target: ItemAlias,
    basin: MagneticDockBasin,
    sample: NormalizedInsetSample,
    cadence: CadenceProfile,
}

impl MagneticDockCase {
    /// Returns the tab item moved by this case.
    #[must_use]
    pub const fn source(self) -> ItemAlias {
        self.source
    }

    /// Returns the item whose pane defines the normalized target point.
    #[must_use]
    pub const fn target(self) -> ItemAlias {
        self.target
    }

    /// Returns the required magnetic basin.
    #[must_use]
    pub const fn basin(self) -> MagneticDockBasin {
        self.basin
    }

    /// Returns the non-guide point normalized to the current target pane.
    #[must_use]
    pub const fn sample(self) -> NormalizedInsetSample {
        self.sample
    }

    /// Returns the bounded input cadence used by this case.
    #[must_use]
    pub const fn cadence(self) -> CadenceProfile {
        self.cadence
    }

    /// Returns the normalized observations required after execution.
    #[must_use]
    pub const fn expected_observations(self) -> [SemanticObservation; 2] {
        [
            SemanticObservation::ItemDocked {
                item: self.source,
                basin: self.basin,
            },
            SemanticObservation::Quiescent,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MagneticDockSample {
    basin: MagneticDockBasin,
    point: NormalizedInsetSample,
}

impl MagneticDockSample {
    const fn new(basin: MagneticDockBasin, x_per_mille: u16, y_per_mille: u16) -> Self {
        Self {
            basin,
            point: NormalizedInsetSample::new(x_per_mille, y_per_mille),
        }
    }
}

/// Finite L03 matrix; this deliberately does not model arbitrary GUI actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MagneticDockScenario {
    id: ScenarioId,
    journey: JourneyId,
    source: ItemAlias,
    target: ItemAlias,
    required_capabilities: &'static [ScenarioCapability],
    samples: &'static [MagneticDockSample],
    cadences: &'static [CadenceProfile],
}

impl MagneticDockScenario {
    const fn new(
        id: ScenarioId,
        journey: JourneyId,
        source: ItemAlias,
        target: ItemAlias,
        required_capabilities: &'static [ScenarioCapability],
        samples: &'static [MagneticDockSample],
        cadences: &'static [CadenceProfile],
    ) -> Self {
        Self {
            id,
            journey,
            source,
            target,
            required_capabilities,
            samples,
            cadences,
        }
    }

    /// Returns the stable semantic scenario identifier.
    #[must_use]
    pub const fn id(self) -> ScenarioId {
        self.id
    }

    /// Returns the behavior-spine journey implemented by this scenario.
    #[must_use]
    pub const fn journey(self) -> JourneyId {
        self.journey
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

    /// Expands the fixed basin/sample/cadence product into executable cases.
    pub fn cases(self) -> impl Iterator<Item = MagneticDockCase> {
        self.cadences.iter().copied().flat_map(move |cadence| {
            self.samples
                .iter()
                .copied()
                .map(move |sample| MagneticDockCase {
                    source: self.source,
                    target: self.target,
                    basin: sample.basin,
                    sample: sample.point,
                    cadence,
                })
        })
    }

    /// Collects normalized outcomes in the same order as [`Self::cases`].
    #[must_use]
    pub fn expected_observations(self) -> Vec<SemanticObservation> {
        self.cases()
            .flat_map(MagneticDockCase::expected_observations)
            .collect()
    }
}

/// Immutable lifecycle scenario consumed only by executors with native facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticLifecycleScenario {
    id: ScenarioId,
    journey: JourneyId,
    required_capabilities: &'static [ScenarioCapability],
    expected_observations: &'static [SemanticObservation],
}

impl SemanticLifecycleScenario {
    const fn new(
        id: ScenarioId,
        journey: JourneyId,
        required_capabilities: &'static [ScenarioCapability],
        expected_observations: &'static [SemanticObservation],
    ) -> Self {
        Self {
            id,
            journey,
            required_capabilities,
            expected_observations,
        }
    }

    /// Returns the stable semantic scenario identifier.
    #[must_use]
    pub const fn id(self) -> ScenarioId {
        self.id
    }

    /// Returns the behavior-spine journey implemented by this scenario.
    #[must_use]
    pub const fn journey(self) -> JourneyId {
        self.journey
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
        journey: JourneyId,
        required_capabilities: &'static [ScenarioCapability],
        steps: &'static [SemanticStep],
    ) -> Self {
        Self {
            id,
            journey,
            required_capabilities,
            steps,
        }
    }

    /// Returns the stable semantic scenario identifier.
    #[must_use]
    pub const fn id(self) -> ScenarioId {
        self.id
    }

    /// Returns the behavior-spine journey implemented by this scenario.
    #[must_use]
    pub const fn journey(self) -> JourneyId {
        self.journey
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

const BASIC_CADENCES: &[CadenceProfile] = &[CadenceProfile::Burst, CadenceProfile::Stepwise];
const L03_CADENCES: &[CadenceProfile] = &[
    CadenceProfile::Stepwise,
    CadenceProfile::Hesitant,
    CadenceProfile::ReleaseWithFinalMove,
    CadenceProfile::Burst,
];
const DOCK_CADENCES: &[CadenceProfile] = &[
    CadenceProfile::Stepwise,
    CadenceProfile::Hesitant,
    CadenceProfile::ReleaseWithFinalMove,
];
const RELEASE_CADENCES: &[CadenceProfile] = &[
    CadenceProfile::Burst,
    CadenceProfile::Hesitant,
    CadenceProfile::ReleaseWithFinalMove,
    CadenceProfile::DiscardBeforeRelease,
];
const RECOVERY_CADENCES: &[CadenceProfile] =
    &[CadenceProfile::Stepwise, CadenceProfile::TargetDisappears];
const TOUCH_CADENCES: &[CadenceProfile] =
    &[CadenceProfile::Stepwise, CadenceProfile::TouchLongPress];
const LOCAL: &[ScenarioCapability] = &[ScenarioCapability::LocalDirectManipulation];
const LOCAL_PRESENTED: &[ScenarioCapability] = &[
    ScenarioCapability::LocalDirectManipulation,
    ScenarioCapability::PresentedCandidate,
];
const LOCAL_DOCK: &[ScenarioCapability] = &[
    ScenarioCapability::LocalDirectManipulation,
    ScenarioCapability::SemanticDockBasins,
    ScenarioCapability::PresentedCandidate,
];
const CONTAINED: &[ScenarioCapability] = &[
    ScenarioCapability::LocalDirectManipulation,
    ScenarioCapability::SemanticDockBasins,
    ScenarioCapability::FullContainedProjection,
];
const TOUCH: &[ScenarioCapability] = &[
    ScenarioCapability::LocalTabSelection,
    ScenarioCapability::LocalItemClose,
    ScenarioCapability::LocalSplitterAdjustment,
    ScenarioCapability::LocalContainedResize,
    ScenarioCapability::TouchLongPress,
];
const NATIVE: &[ScenarioCapability] = &[
    ScenarioCapability::NativeWindowLifecycle,
    ScenarioCapability::NativeFirstLive,
    ScenarioCapability::NativeRetirement,
    ScenarioCapability::NativeQuiescence,
];
const PHYSICAL: &[ScenarioCapability] = &[
    ScenarioCapability::NativeWindowLifecycle,
    ScenarioCapability::NativePhysicalAuthority,
    ScenarioCapability::NativeRetirement,
    ScenarioCapability::NativeQuiescence,
];

/// Fixed U1 journey directory; detailed checkpoints and oracle decisions are documented.
pub const JOURNEY_CATALOG: &[JourneySpec] = &[
    JourneySpec::new(
        JourneyId::L01,
        "tab select and drag threshold",
        BASIC_CADENCES,
        LOCAL,
    ),
    JourneySpec::new(
        JourneyId::L02,
        "local tab reorder",
        RELEASE_CADENCES,
        LOCAL_PRESENTED,
    ),
    JourneySpec::new(
        JourneyId::L03,
        "center and edge docking basins",
        L03_CADENCES,
        LOCAL_DOCK,
    ),
    JourneySpec::new(
        JourneyId::L04,
        "contained move resize and redock",
        DOCK_CADENCES,
        CONTAINED,
    ),
    JourneySpec::new(
        JourneyId::L05,
        "cancel stale and target-loss recovery",
        RECOVERY_CADENCES,
        LOCAL_PRESENTED,
    ),
    JourneySpec::new(
        JourneyId::L06,
        "presentation-qualified release",
        RELEASE_CADENCES,
        LOCAL_DOCK,
    ),
    JourneySpec::new(
        JourneyId::L07,
        "group and chrome ownership",
        BASIC_CADENCES,
        LOCAL,
    ),
    JourneySpec::new(
        JourneyId::L08,
        "input and layout command convergence",
        TOUCH_CADENCES,
        TOUCH,
    ),
    JourneySpec::new(
        JourneyId::N01,
        "programmatic native lifecycle",
        BASIC_CADENCES,
        NATIVE,
    ),
    JourneySpec::new(
        JourneyId::P01,
        "physical native drag-out and redock",
        DOCK_CADENCES,
        PHYSICAL,
    ),
];

const MAGNETIC_DOCK_SAMPLES: &[MagneticDockSample] = &[
    MagneticDockSample::new(MagneticDockBasin::Center, 280, 280),
    MagneticDockSample::new(MagneticDockBasin::Center, 720, 280),
    MagneticDockSample::new(MagneticDockBasin::Center, 720, 720),
    MagneticDockSample::new(MagneticDockBasin::North, 200, 80),
    MagneticDockSample::new(MagneticDockBasin::North, 800, 120),
    MagneticDockSample::new(MagneticDockBasin::North, 300, 160),
    MagneticDockSample::new(MagneticDockBasin::South, 200, 840),
    MagneticDockSample::new(MagneticDockBasin::South, 800, 880),
    MagneticDockSample::new(MagneticDockBasin::South, 300, 920),
    MagneticDockSample::new(MagneticDockBasin::East, 840, 250),
    MagneticDockSample::new(MagneticDockBasin::East, 880, 750),
    MagneticDockSample::new(MagneticDockBasin::East, 920, 300),
    MagneticDockSample::new(MagneticDockBasin::West, 80, 250),
    MagneticDockSample::new(MagneticDockBasin::West, 120, 750),
    MagneticDockSample::new(MagneticDockBasin::West, 160, 300),
];

/// Shared L03 journey executed by both local conformance adapters.
pub const MAGNETIC_DOCKING_BASINS: MagneticDockScenario = MagneticDockScenario::new(
    ScenarioId::MagneticDockingBasins,
    JourneyId::L03,
    ItemAlias::Secondary,
    ItemAlias::Primary,
    LOCAL_DOCK,
    MAGNETIC_DOCK_SAMPLES,
    L03_CADENCES,
);

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
    JourneyId::L08,
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
        JourneyId::N01,
        NATIVE_LIFECYCLE_CAPABILITIES,
        NATIVE_LIFECYCLE_OBSERVATIONS,
    );
