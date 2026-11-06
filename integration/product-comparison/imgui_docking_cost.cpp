// Renderer-free docking commands from the pinned, unmodified upstream source.
// Reference: d88f238d1d363ef387382b2e29ad12cf96f826ac (1.93.1 WIP).
// Include imgui.cpp once here; link imgui_draw/tables/widgets.cpp separately.
#include "imgui.cpp"

#include <algorithm>
#include <array>
#include <atomic>
#include <charconv>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <initializer_list>
#include <iostream>
#include <limits>
#include <stdexcept>
#include <string>
#include <string_view>
#include <system_error>
#include <unordered_map>
#include <unordered_set>
#include <utility>
#include <vector>

static_assert(IMGUI_VERSION_NUM == 19301, "Use the pinned Dear ImGui reference.");

namespace {

using Clock = std::chrono::steady_clock;
using Word = std::uint64_t;
constexpr float kWidths[] = {16380.0f, 16444.0f};
constexpr float kHeight = 16380.0f;
constexpr float kSeparator = 4.0f;
constexpr char kProduct[] = "dear_imgui";

void require(bool condition, const char* message) {
    if (!condition)
        throw std::runtime_error(message);
}

enum class Scenario { Resize, CenterMove, EdgeSplit, CrossViewMove };
constexpr Scenario kScenarios[] = {Scenario::Resize, Scenario::CenterMove,
                                   Scenario::EdgeSplit, Scenario::CrossViewMove};

const char* scenario_name(Scenario scenario) {
    switch (scenario) {
    case Scenario::Resize: return "resize";
    case Scenario::CenterMove: return "center_move";
    case Scenario::EdgeSplit: return "edge_split";
    case Scenario::CrossViewMove: return "cross_view_move";
    }
    throw std::runtime_error("unknown docking scenario");
}

std::size_t count_value(std::string_view text) {
    std::size_t value = 0;
    const auto parsed = std::from_chars(text.data(), text.data() + text.size(), value);
    require(!text.empty() && parsed.ec == std::errc() && parsed.ptr == text.data() + text.size(),
            "environment count must be an unsigned decimal integer");
    return value;
}

std::size_t count_setting(const char* name, std::size_t fallback, bool allow_zero) {
    const char* configured = std::getenv(name);
    const std::size_t value = configured ? count_value(configured) : fallback;
    require(value <= 10000 && (allow_zero || value != 0), "sample/warmup count out of range");
    return value;
}

std::vector<std::size_t> list_setting(const char* name, const char* fallback, bool panes) {
    const char* configured = std::getenv(name);
    const std::string_view text(configured ? configured : fallback);
    std::vector<std::size_t> values;
    std::size_t start = 0;
    while (true) {
        const std::size_t comma = text.find(',', start);
        const std::size_t end = comma == std::string_view::npos ? text.size() : comma;
        const std::size_t value = count_value(text.substr(start, end - start));
        require(panes ? (value == 16 || value == 128 || value == 1024)
                      : (value == 1 || value == 4), "unsupported pane/view count");
        require(std::find(values.begin(), values.end(), value) == values.end(),
                "duplicate pane/view count");
        values.push_back(value);
        if (comma == std::string_view::npos)
            return values;
        start = comma + 1;
    }
}

struct Config {
    std::size_t samples = count_setting("DOCKSPACE_DOCK_COST_SAMPLES", 48, false);
    std::size_t warmup = count_setting("DOCKSPACE_DOCK_COST_WARMUP", 8, true);
    std::vector<std::size_t> panes = list_setting("DOCKSPACE_DOCK_COST_PANES", "16,128,1024", true);
    std::vector<std::size_t> views = list_setting("DOCKSPACE_DOCK_COST_VIEWS", "1,4", false);
    std::string one_case;
    std::filesystem::path state_dir;

    Config() {
        if (const char* configured = std::getenv("DOCKSPACE_DOCK_COST_CASE")) {
            one_case = configured;
            bool known = false;
            for (const Scenario scenario : kScenarios)
                known |= one_case == scenario_name(scenario);
            require(known, "unsupported DOCKSPACE_DOCK_COST_CASE");
            if (one_case == "cross_view_move")
                require(std::find(views.begin(), views.end(), 4) != views.end(),
                        "cross_view_move requires four views");
        }
        if (const char* configured = std::getenv("DOCKSPACE_DOCK_COST_STATE_DIR")) {
            require(*configured != '\0', "state directory must not be empty");
            state_dir = configured;
            std::filesystem::create_directories(state_dir);
        }
    }
};

struct Context {
    ImGuiContext* value = ImGui::CreateContext();

    Context() {
        ImGui::SetCurrentContext(value);
        value->IO.IniFilename = nullptr;
        value->IO.LogFilename = nullptr;
        value->IO.ConfigFlags |= ImGuiConfigFlags_DockingEnable;
        value->Style.DockingSeparatorSize = kSeparator;
        value->Style.WindowMinSize = ImVec2(0.0f, 0.0f);
    }
    ~Context() { ImGui::DestroyContext(value); }
    Context(const Context&) = delete;
    Context& operator=(const Context&) = delete;
};

struct Fixture {
    // The context exclusively owns nodes, windows, and tab bars. These handles do not own them.
    Context context;
    const std::size_t panes;
    const std::size_t views;
    std::vector<ImGuiID> root_ids;
    std::vector<ImGuiWindow*> items;
    std::unordered_map<const ImGuiWindow*, Word> stable_items;

    Fixture(std::size_t pane_count, std::size_t view_count, float initial_width)
        : panes(pane_count), views(view_count) {
        require(panes % views == 0, "panes must divide equally among views");
        root_ids.reserve(views);
        items.reserve(panes * 2);
        stable_items.reserve(panes * 2);
        for (std::size_t view = 0; view < views; ++view) {
            ImGuiDockNode* root = ImGui::DockContextAddNode(context.value, 0);
            root->SetLocalFlags(ImGuiDockNodeFlags_DockSpace);
            root->LastFrameAlive = context.value->FrameCount;
            root->Pos = ImVec2(0.0f, 0.0f);
            root->Size = root->SizeRef = ImVec2(initial_width, kHeight);
            root_ids.push_back(root->ID);
            populate(root->ID, panes / views, ImGuiAxis_X);
        }
        for (const ImGuiID id : root_ids) {
            ImGuiDockNode* root = ImGui::DockContextFindNodeByID(context.value, id);
            ImGui::DockNodeUpdateForRootNode(root);
            ImGui::DockNodeTreeUpdatePosSize(root, ImVec2(0.0f, 0.0f), ImVec2(initial_width, kHeight));
        }
        std::unordered_set<ImGuiID> window_ids;
        std::unordered_set<ImGuiID> tab_ids;
        for (ImGuiWindow* item : items) {
            require(window_ids.insert(item->ID).second && tab_ids.insert(item->TabId).second,
                    "fixture native identity collision");
            require(ImGui::FindWindowByID(item->ID) == item, "fixture window registration failed");
        }
        require(items.size() == panes * 2, "wrong fixture item count");
    }

    void populate(ImGuiID id, std::size_t leaves, ImGuiAxis axis) {
        if (leaves > 1) {
            ImGuiID first = 0, second = 0;
            ImGui::DockBuilderSplitNode(id, axis == ImGuiAxis_X ? ImGuiDir_Left : ImGuiDir_Up,
                                       0.5f, &first, &second);
            const ImGuiAxis next = axis == ImGuiAxis_X ? ImGuiAxis_Y : ImGuiAxis_X;
            populate(first, leaves / 2, next);
            populate(second, leaves / 2, next);
            return;
        }
        ImGuiDockNode* node = ImGui::DockContextFindNodeByID(context.value, id);
        for (int tab = 0; tab < 2; ++tab) {
            const Word stable_id = static_cast<Word>(items.size() + 1);
            const std::string name = "DockingKernel/Item" + std::to_string(stable_id);
            require(ImGui::FindWindowByName(name.c_str()) == nullptr, "fixture window ID collision");
            ImGuiWindow* window = CreateNewWindow(name.c_str(), ImGuiWindowFlags_NoSavedSettings);
            window->WasActive = window->Active = true;
            window->Size = window->SizeFull = node->Size;
            window->Pos = node->Pos;
            window->DockOrder = static_cast<short>(tab);
            ImGui::UpdateWindowParentAndRootLinks(window, window->Flags, nullptr);
            items.push_back(window);
            require(stable_items.emplace(window, stable_id).second, "duplicate fixture window");
            ImGui::DockNodeAddWindow(node, window, true);
        }
        require(node->TabBar != nullptr && node->TabBar->Tabs.Size == 2, "fixture tab bar missing");
        node->SelectedTabId = node->TabBar->SelectedTabId = node->Windows[0]->TabId;
        node->TabBar->NextSelectedTabId = 0;
        for (ImGuiTabItem& tab : node->TabBar->Tabs)
            tab.Flags &= ~ImGuiTabItemFlags_Unsorted;
    }

    Word item_id(const ImGuiWindow* window) const {
        const auto found = stable_items.find(window);
        require(found != stable_items.end(), "unknown window identity in normalized state");
        return found->second;
    }
};

Word integral(float value) {
    require(std::isfinite(value) && value >= 0.0f && value <= 65536.0f && std::trunc(value) == value,
            "snapshot geometry must be exact nonnegative integers");
    return static_cast<Word>(value);
}

Word selected_intent(const Fixture& fixture, const ImGuiDockNode* node) {
    if (!node->TabBar) {
        require(node->Windows.Size <= 1, "multi-item leaf has no native tab bar");
        return node->Windows.empty() ? 0 : fixture.item_id(node->Windows[0]);
    }
    const ImGuiID native_id = node->TabBar->NextSelectedTabId
        ? node->TabBar->NextSelectedTabId : node->TabBar->SelectedTabId;
    for (const ImGuiTabItem& tab : node->TabBar->Tabs)
        if (tab.ID == native_id) {
            return fixture.item_id(tab.Window);
        }
    require(node->Windows.empty() && native_id == 0, "selected intent is not a leaf member");
    return 0;
}

struct Counts {
    std::size_t nodes = 0;
    std::size_t leaves = 0;
    std::size_t splits = 0;
    std::size_t items = 0;
    std::size_t roots = 0;
};

struct Snapshot {
    std::vector<Word> words;
    Counts counts;
};

void export_node(const Fixture& fixture, const ImGuiDockNode* node, Word view, Word path,
                 Snapshot& snapshot) {
    require(node != nullptr, "missing node during export");
    const bool leaf = node->IsLeafNode();
    require(leaf || node->SplitAxis == ImGuiAxis_X || node->SplitAxis == ImGuiAxis_Y,
            "invalid split axis during export");
    const Word kind = leaf ? 0 : node->SplitAxis == ImGuiAxis_X ? 1 : 2;
    const Word item_count = leaf ? static_cast<Word>(node->Windows.Size) : 0;
    const Word selected = leaf ? selected_intent(fixture, node) : 0;
    for (const Word word : {view, path, kind, integral(node->Pos.x), integral(node->Pos.y),
                            integral(node->Size.x), integral(node->Size.y), item_count, selected})
        snapshot.words.push_back(word);
    if (leaf) {
        if (node->TabBar) {
            for (const ImGuiTabItem& tab : node->TabBar->Tabs)
                snapshot.words.push_back(fixture.item_id(tab.Window));
        } else if (!node->Windows.empty()) {
            snapshot.words.push_back(fixture.item_id(node->Windows[0]));
        }
        return;
    }
    require(path <= (std::numeric_limits<Word>::max() - 1) / 2, "snapshot path overflow");
    export_node(fixture, node->ChildNodes[0], view, path * 2, snapshot);
    export_node(fixture, node->ChildNodes[1], view, path * 2 + 1, snapshot);
}

struct Request {
    std::array<ImGuiDockNode*, 4> roots{};
    ImGuiDockRequest dock;
    bool is_dock = false;
};

Request capture_request(const Fixture& fixture, Scenario scenario) {
    Request request;
    for (std::size_t view = 0; view < fixture.views; ++view) {
        request.roots[view] = ImGui::DockContextFindNodeByID(fixture.context.value, fixture.root_ids[view]);
        require(request.roots[view] && request.roots[view]->IsRootNode(), "missing request root");
    }
    if (scenario == Scenario::Resize)
        return request;
    const std::size_t target_pane = scenario == Scenario::CrossViewMove
        ? fixture.panes - 1 : fixture.panes / fixture.views - 1;
    ImGuiWindow* source = fixture.items.at(1);
    ImGuiWindow* target = fixture.items.at(target_pane * 2);
    require(source != target && source->DockNode && target->DockNode &&
            source->DockNode != target->DockNode, "invalid command operands");
    require(ImGui::DockNodeIsDropAllowed(target, source), "native docking policy rejected fixture");
    request.is_dock = true;
    request.dock.Type = ImGuiDockRequestType_Dock;
    request.dock.DockTargetNode = target->DockNode;
    request.dock.DockPayload = source;
    if (scenario == Scenario::EdgeSplit) {
        require(target->DockNode->ParentNode != nullptr, "edge target has no parent");
        const ImGuiAxis parent = static_cast<ImGuiAxis>(target->DockNode->ParentNode->SplitAxis);
        require(parent == ImGuiAxis_X || parent == ImGuiAxis_Y, "invalid target parent axis");
        request.dock.DockSplitDir = parent == ImGuiAxis_Y ? ImGuiDir_Left : ImGuiDir_Up;
        request.dock.DockSplitRatio = 0.5f;
    }
    return request;
}

Clock::time_point timestamp() {
    std::atomic_signal_fence(std::memory_order_seq_cst);
    const auto value = Clock::now();
    std::atomic_signal_fence(std::memory_order_seq_cst);
    return value;
}

Word duration_ns(Clock::time_point start, Clock::time_point end) {
    return static_cast<Word>(std::chrono::duration_cast<std::chrono::nanoseconds>(end - start).count());
}

struct Observation {
    Snapshot snapshot;
    Word request_ns = 0;
    Word mutation_ns = 0;
    Word layout_ns = 0;
    Word export_ns = 0;
    Word operation_owned_ns = 0;
    Word state_digest = 0;
};

Observation measure(const Fixture& fixture, Scenario scenario, float width) {
    Observation result;
    const auto start = timestamp();
    Request request = capture_request(fixture, scenario);
    const auto captured = timestamp();
    if (request.is_dock)
        ImGui::DockContextProcessDock(fixture.context.value, &request.dock);
    // All-root flags, cleanup, and derived topology metadata belong to mutation_ns.
    for (std::size_t view = 0; view < fixture.views; ++view)
        ImGui::DockNodeUpdateForRootNode(request.roots[view]);
    const auto mutated = timestamp();
    for (std::size_t view = 0; view < fixture.views; ++view)
        ImGui::DockNodeTreeUpdatePosSize(request.roots[view], ImVec2(0.0f, 0.0f), ImVec2(width, kHeight));
    const auto laid_out = timestamp();
    result.snapshot.words.reserve(fixture.panes * 32 + fixture.views * 16);
    for (std::size_t view = 0; view < fixture.views; ++view)
        export_node(fixture, request.roots[view], static_cast<Word>(view + 1), 1, result.snapshot);
    const auto exported = timestamp();
    result.request_ns = duration_ns(start, captured);
    result.mutation_ns = duration_ns(captured, mutated);
    result.layout_ns = duration_ns(mutated, laid_out);
    result.export_ns = duration_ns(laid_out, exported);
    result.operation_owned_ns = duration_ns(start, exported);
    return result;
}

Word digest(const std::vector<Word>& words) {
    Word value = UINT64_C(0xcbf29ce484222325);
    for (const Word word : words)
        for (unsigned byte = 0; byte < 8; ++byte)
            value = (value ^ ((word >> (byte * 8)) & 255)) * UINT64_C(0x100000001b3);
    return value;
}

struct Rect {
    Word x, y, width, height;
};

std::array<Rect, 2> halve(Rect rect, ImGuiAxis axis) {
    const Word extent = axis == ImGuiAxis_X ? rect.width : rect.height;
    require(extent > 4 && (extent - 4) % 2 == 0, "common geometry oracle requires integral halves");
    const Word half = (extent - 4) / 2;
    std::array<Rect, 2> children = {rect, rect};
    if (axis == ImGuiAxis_X) {
        children[0].width = children[1].width = half;
        children[1].x += half + 4;
    } else {
        children[0].height = children[1].height = half;
        children[1].y += half + 4;
    }
    return children;
}

struct StateCheck {
    const Fixture& fixture;
    const Scenario scenario;
    const Snapshot* snapshot;
    std::size_t cursor = 0;
    Counts counts;
    std::vector<bool> seen_items;
    std::unordered_set<const ImGuiDockNode*> seen_nodes;

    StateCheck(const Fixture& value, Scenario selected, const Snapshot* owned)
        : fixture(value), scenario(selected), snapshot(owned), seen_items(value.panes * 2, false) {}

    void word(Word expected) {
        if (snapshot) {
            require(cursor < snapshot->words.size() && snapshot->words[cursor] == expected,
                    "owned snapshot differs from complete fixture oracle");
            ++cursor;
        }
    }

    void header(const ImGuiDockNode* node, const ImGuiDockNode* parent, Word view, Word path,
                Word kind, Rect bounds, Word member_count, Word selected) {
        require(node && seen_nodes.insert(node).second, "missing or repeated live node");
        require(node->ParentNode == parent && node->HostWindow == nullptr && !node->IsCentralNode(),
                "unexpected parent, host, or central node in common subset");
        require(ImGui::DockContextFindNodeByID(fixture.context.value, node->ID) == node,
                "live node missing from context registry");
        require(node->IsVisible, "common fixture node became invisible");
        require(node->Pos.x == static_cast<float>(bounds.x) &&
                node->Pos.y == static_cast<float>(bounds.y) &&
                node->Size.x == static_cast<float>(bounds.width) &&
                node->Size.y == static_cast<float>(bounds.height), "complete node geometry differs");
        require(node->IsLeafNode() == (kind == 0), "wrong leaf/split topology");
        if (kind != 0)
            require(node->SplitAxis == (kind == 1 ? ImGuiAxis_X : ImGuiAxis_Y) && node->Windows.empty(),
                    "wrong split axis or split membership");
        for (const Word value : {view, path, kind, bounds.x, bounds.y, bounds.width,
                                 bounds.height, member_count, selected})
            word(value);
        ++counts.nodes;
    }

    void leaf(const ImGuiDockNode* node, const ImGuiDockNode* parent, Word view, Word path,
              Rect bounds, std::initializer_list<Word> members, Word selected) {
        header(node, parent, view, path, 0, bounds, static_cast<Word>(members.size()), selected);
        require(node->Windows.Size == static_cast<int>(members.size()), "wrong live leaf member count");
        require(selected_intent(fixture, node) == selected, "wrong command selection intent");
        if (node->TabBar)
            require(node->TabBar->Tabs.Size == node->Windows.Size, "live tab/window counts differ");
        else
            require(members.size() == 1, "missing native multi-item tab bar");
        std::size_t order = 0;
        for (const Word id : members) {
            require(id != 0 && id <= fixture.items.size(), "invalid expected item ID");
            ImGuiWindow* window = fixture.items[static_cast<std::size_t>(id - 1)];
            require(!seen_items[static_cast<std::size_t>(id - 1)], "item appears in multiple leaves");
            seen_items[static_cast<std::size_t>(id - 1)] = true;
            require(window->DockNode == node && window->DockId == node->ID &&
                    fixture.item_id(window) == id, "window docking ownership differs");
            int membership = 0;
            for (const ImGuiWindow* member : node->Windows)
                membership += member == window ? 1 : 0;
            require(membership == 1, "window missing or duplicated in native membership");
            if (node->TabBar) {
                const ImGuiTabItem& tab = node->TabBar->Tabs[static_cast<int>(order)];
                require(tab.Window == window && tab.ID == window->TabId, "native tab order/identity differs");
            } else {
                require(node->Windows[0] == window, "wrong surviving singleton item");
            }
            require(window->DrawList->VtxBuffer.empty() && window->DrawList->IdxBuffer.empty(),
                    "renderer-free kernel unexpectedly emitted geometry");
            word(id);
            ++order;
        }
        ++counts.leaves;
        counts.items += members.size();
    }

    void tree(const ImGuiDockNode* node, const ImGuiDockNode* parent, Word view, Word path,
              std::size_t remaining, ImGuiAxis axis, Rect bounds, std::size_t first_pane) {
        if (remaining > 1) {
            header(node, parent, view, path, axis == ImGuiAxis_X ? 1 : 2, bounds, 0, 0);
            ++counts.splits;
            const auto halves = halve(bounds, axis);
            const ImGuiAxis next = axis == ImGuiAxis_X ? ImGuiAxis_Y : ImGuiAxis_X;
            tree(node->ChildNodes[0], node, view, path * 2, remaining / 2, next, halves[0], first_pane);
            tree(node->ChildNodes[1], node, view, path * 2 + 1, remaining / 2, next, halves[1],
                 first_pane + remaining / 2);
            return;
        }
        const Word first = static_cast<Word>(first_pane * 2 + 1);
        const std::size_t target = scenario == Scenario::CrossViewMove
            ? fixture.panes - 1 : fixture.panes / fixture.views - 1;
        if (scenario == Scenario::EdgeSplit && first_pane == target) {
            // The inserted axis alternates from the old parent; payload is left/top child 0.
            header(node, parent, view, path, axis == ImGuiAxis_X ? 1 : 2, bounds, 0, 0);
            ++counts.splits;
            const auto halves = halve(bounds, axis);
            leaf(node->ChildNodes[0], node, view, path * 2, halves[0], {2}, 2);
            leaf(node->ChildNodes[1], node, view, path * 2 + 1, halves[1], {first, first + 1}, first);
        } else if (scenario != Scenario::Resize && first_pane == 0) {
            leaf(node, parent, view, path, bounds, {first}, first);
        } else if ((scenario == Scenario::CenterMove || scenario == Scenario::CrossViewMove) &&
                   first_pane == target) {
            leaf(node, parent, view, path, bounds, {first, first + 1, 2}, 2);
        } else {
            leaf(node, parent, view, path, bounds, {first, first + 1}, first);
        }
    }

    void all(float width) {
        for (std::size_t view = 0; view < fixture.views; ++view) {
            const ImGuiDockNode* root = ImGui::DockContextFindNodeByID(fixture.context.value,
                                                                     fixture.root_ids[view]);
            require(root && root->IsDockSpace(), "logical view root is not a dockspace");
            tree(root, nullptr, static_cast<Word>(view + 1), 1, fixture.panes / fixture.views,
                 ImGuiAxis_X, Rect{0, 0, integral(width), integral(kHeight)},
                 view * (fixture.panes / fixture.views));
            ++counts.roots;
        }
        const std::size_t extra = scenario == Scenario::EdgeSplit ? 1 : 0;
        require(counts.nodes == fixture.panes * 2 - fixture.views + extra * 2 &&
                counts.leaves == fixture.panes + extra &&
                counts.splits == fixture.panes - fixture.views + extra &&
                counts.items == fixture.panes * 2 && counts.roots == fixture.views,
                "complete topology/item totals differ");
        require(std::all_of(seen_items.begin(), seen_items.end(), [](bool seen) { return seen; }),
                "an item was lost from all roots");
        std::size_t registered_nodes = 0;
        for (const ImGuiStoragePair& entry : fixture.context.value->DockContext.Nodes.Data)
            if (entry.val_p) {
                require(seen_nodes.count(static_cast<ImGuiDockNode*>(entry.val_p)) == 1,
                        "extra context-owned node outside logical roots");
                ++registered_nodes;
            }
        require(registered_nodes == counts.nodes, "context node registry count differs");
        require(fixture.context.value->Windows.Size == static_cast<int>(fixture.items.size()),
                "unexpected context-owned window");
        require(fixture.context.value->Font == nullptr && fixture.context.value->FontBaked == nullptr,
                "renderer-free kernel unexpectedly activated a font");
        if (snapshot) {
            require(cursor == snapshot->words.size(), "snapshot contains extra normalized words");
        }
    }
};

Observation run_sample(std::size_t panes, std::size_t views, Scenario scenario,
                       std::size_t iteration) {
    const std::size_t variant = iteration % 2;
    const float output_width = scenario == Scenario::Resize ? kWidths[variant] : kWidths[0];
    const float initial_width = scenario == Scenario::Resize ? kWidths[variant ^ 1] : kWidths[0];
    Observation observation;
    {
        Fixture fixture(panes, views, initial_width);
        StateCheck(fixture, Scenario::Resize, nullptr).all(initial_width);
        observation = measure(fixture, scenario, output_width);
        StateCheck final_state(fixture, scenario, &observation.snapshot);
        final_state.all(output_width);
        observation.snapshot.counts = final_state.counts;
        observation.state_digest = digest(observation.snapshot.words);
        // Fixture/context destruction follows observation and remains outside all timers.
    }
    require(digest(observation.snapshot.words) == observation.state_digest,
            "owned snapshot did not survive context destruction");
    return observation;
}

void dump_state(const std::filesystem::path& directory, std::size_t panes, std::size_t views,
                Scenario scenario, const Snapshot& snapshot) {
    if (directory.empty())
        return;
    const std::string filename = std::string(kProduct) + '-' + std::to_string(panes) + '-' +
        std::to_string(views) + '-' + scenario_name(scenario) + ".txt";
    std::ofstream output(directory / filename);
    require(output.is_open(), "cannot open normalized state dump");
    for (const Word word : snapshot.words)
        output << word << '\n';
    output.close();
    require(!output.fail(), "cannot finish normalized state dump");
}

void emit(std::size_t panes, std::size_t views, Scenario scenario, std::size_t sample,
          std::size_t iteration, const Observation& observation) {
    const Counts& count = observation.snapshot.counts;
    const Word width = integral(scenario == Scenario::Resize ? kWidths[iteration % 2] : kWidths[0]);
    std::cout << kProduct << ',' << panes << ',' << views << ',' << scenario_name(scenario) << ',' << sample
              << ',' << observation.request_ns << ',' << observation.mutation_ns
              << ',' << observation.layout_ns << ',' << observation.export_ns
              << ',' << observation.operation_owned_ns << ',' << count.nodes
              << ',' << count.leaves << ',' << count.splits << ',' << count.items
              << ',' << count.roots << ',' << width << ',' << integral(kHeight) << ','
              << std::hex << std::nouppercase << std::setw(16) << std::setfill('0')
              << observation.state_digest << std::dec << '\n';
}

} // namespace

int main() {
    try {
        const Config config;
        std::cout << "product,panes,views,case,sample,request_ns,mutation_ns,layout_ns,export_ns,"
                     "operation_owned_ns,nodes,leaves,splits,items,roots,width,height,state_digest\n";
        for (const std::size_t panes : config.panes)
            for (const std::size_t views : config.views)
                for (const Scenario scenario : kScenarios) {
                    if (scenario == Scenario::CrossViewMove && views != 4)
                        continue;
                    if (!config.one_case.empty() && config.one_case != scenario_name(scenario))
                        continue;
                    for (std::size_t iteration = 0; iteration < config.warmup + config.samples; ++iteration) {
                        const Observation observation = run_sample(panes, views, scenario, iteration);
                        if (iteration < config.warmup)
                            continue;
                        const std::size_t sample = iteration - config.warmup;
                        if (sample == 0)
                            dump_state(config.state_dir, panes, views, scenario, observation.snapshot);
                        emit(panes, views, scenario, sample, iteration, observation);
                    }
                }
        require(std::cout.good(), "cannot finish docking CSV output");
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
