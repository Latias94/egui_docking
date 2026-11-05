// CPU-only product workload for the unmodified Dear ImGui docking reference.
// Reference: d88f238d1d363ef387382b2e29ad12cf96f826ac (1.93.1 WIP).
#include "imgui.h"
#include "imgui_internal.h"

#include <algorithm>
#include <array>
#include <charconv>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <exception>
#include <iostream>
#include <limits>
#include <memory>
#include <sstream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <system_error>
#include <vector>

static_assert(IMGUI_VERSION_NUM == 19301, "Use the pinned Dear ImGui reference.");

namespace {

constexpr float kWidth = 16384.0f;
constexpr float kResizedWidth = 16448.0f;
constexpr float kHeight = 16384.0f;
constexpr float kDeltaTime = 1.0f / 60.0f;
constexpr ImGuiID kDockspace = 0x75e2a418;
constexpr std::size_t kReadinessLimit = 64;

[[noreturn]] void fail(const std::string& message) {
    throw std::runtime_error(message);
}

void require(bool condition, const char* message) {
    if (!condition) {
        fail(message);
    }
}

struct Config {
    std::size_t samples = 24;
    std::size_t warmup = 6;
    std::vector<std::size_t> panes = {16, 128, 1024};
    bool help = false;
};

std::size_t parse_count(std::string_view text, const char* option, bool allow_zero) {
    std::size_t value = 0;
    const auto result = std::from_chars(text.data(), text.data() + text.size(), value);
    if (result.ec != std::errc() || result.ptr != text.data() + text.size() ||
        (!allow_zero && value == 0)) {
        fail(std::string(option) + " requires " +
             (allow_zero ? "a nonnegative integer" : "a positive integer"));
    }
    return value;
}

Config parse_args(int argc, char** argv) {
    Config config;
    for (int index = 1; index < argc; ++index) {
        const std::string_view option = argv[index];
        if (option == "--help") {
            config.help = true;
            continue;
        }
        if (option != "--samples" && option != "--warmup" && option != "--panes") {
            fail("unknown argument: " + std::string(option));
        }
        if (++index == argc) {
            fail("missing value for " + std::string(option));
        }
        const std::string_view value = argv[index];
        if (option == "--samples") {
            config.samples = parse_count(value, "--samples", false);
        } else if (option == "--warmup") {
            config.warmup = parse_count(value, "--warmup", true);
        } else {
            config.panes.clear();
            std::size_t start = 0;
            do {
                const std::size_t comma = value.find(',', start);
                const std::size_t end = comma == std::string_view::npos ? value.size() : comma;
                const std::size_t panes =
                    parse_count(value.substr(start, end - start), "--panes", false);
                require(panes <= static_cast<std::size_t>(std::numeric_limits<int>::max() / 2),
                        "pane count exceeds Dear ImGui's representable window count");
                require(std::find(config.panes.begin(), config.panes.end(), panes) ==
                            config.panes.end(),
                        "--panes must not contain duplicate counts");
                config.panes.push_back(panes);
                if (comma == std::string_view::npos) {
                    break;
                }
                start = comma + 1;
            } while (true);
        }
    }
    return config;
}

enum class Scenario { Idle, TabClick, SurfaceResize };
enum class MouseAction { None, Move, Press, Release };

const char* scenario_name(Scenario scenario) {
    switch (scenario) {
    case Scenario::Idle:
        return "idle";
    case Scenario::TabClick:
        return "tab_click";
    case Scenario::SurfaceResize:
        return "surface_resize";
    }
    fail("unknown scenario");
}

struct Sample {
    std::uint64_t frames = 1;
    std::uint64_t elapsed_ns = 0;
    std::uint64_t passes = 1;
    std::uint64_t content_calls = 0;
    std::uint64_t visible_contents = 0;
    std::uint64_t vertices = 0;
    std::uint64_t indices = 0;
    std::uint64_t draw_commands = 0;

    void add(const Sample& other) {
        frames += other.frames;
        elapsed_ns += other.elapsed_ns;
        passes += other.passes;
        content_calls += other.content_calls;
        visible_contents += other.visible_contents;
        vertices += other.vertices;
        indices += other.indices;
        draw_commands += other.draw_commands;
    }
};

struct Pane {
    ImGuiID node = 0;
    std::array<std::string, 2> titles;
    std::array<ImGuiWindow*, 2> windows = {nullptr, nullptr};
    std::array<bool, 2> submitted = {false, false};
    std::array<ImRect, 2> squares;
};

bool has_area(const ImRect& rect) {
    return std::isfinite(rect.Min.x) && std::isfinite(rect.Min.y) &&
           std::isfinite(rect.Max.x) && std::isfinite(rect.Max.y) &&
           rect.GetWidth() > 0.0f && rect.GetHeight() > 0.0f;
}

class ImGuiProduct {
public:
    explicit ImGuiProduct(std::size_t count)
        : context_(ImGui::CreateContext(), ImGui::DestroyContext), panes_(count) {
        require(context_ != nullptr, "failed to create Dear ImGui context");
        require(IMGUI_CHECKVERSION(), "Dear ImGui header/library layout mismatch");
        ImGuiIO& io = ImGui::GetIO();
        io.ConfigFlags |= ImGuiConfigFlags_DockingEnable;
        io.IniFilename = nullptr;
        io.LogFilename = nullptr;
        io.DisplaySize = ImVec2(kWidth, kHeight);
        io.DisplayFramebufferScale = ImVec2(1.0f, 1.0f);
        io.DeltaTime = kDeltaTime;
        io.BackendFlags |=
            ImGuiBackendFlags_RendererHasTextures | ImGuiBackendFlags_RendererHasVtxOffset;
        io.BackendPlatformName = "product_comparison_cpu";
        io.BackendRendererName = "product_comparison_texture_ack";
        ImGui::GetStyle().FontSizeBase = 14.0f;
        ImGui::GetStyle().WindowMinSize = ImVec2(64.0f, 48.0f);

        for (std::size_t index = 0; index < count; ++index) {
            for (std::size_t tab = 0; tab < 2; ++tab) {
                char title[64];
                const int length = std::snprintf(title, sizeof(title), "Pane %04zu %c",
                                                 index + 1, tab == 0 ? 'A' : 'B');
                require(length > 0 && static_cast<std::size_t>(length) < sizeof(title),
                        "pane title exceeds its setup buffer");
                panes_[index].titles[tab] = title;
            }
        }

        Sample frame = run_frame(kWidth, MouseAction::None, ImVec2(), true);
        std::string issue;
        for (std::size_t attempt = 0; attempt < kReadinessLimit; ++attempt) {
            issue = frame_issue(frame);
            if (issue.empty()) {
                validate_mesh(frame);
                return;
            }
            if (attempt + 1 < kReadinessLimit) {
                frame = run_frame(kWidth);
            }
        }
        fail("panes=" + std::to_string(count) + " did not become ready within " +
             std::to_string(kReadinessLimit) + " frames: " + issue);
    }

    Sample sample(Scenario scenario) {
        switch (scenario) {
        case Scenario::Idle:
            return checked_frame(width_);
        case Scenario::SurfaceResize:
            return checked_frame(width_ == kWidth ? kResizedWidth : kWidth);
        case Scenario::TabClick:
            return click_inactive_tab();
        }
        fail("unknown scenario");
    }

private:
    using Context = std::unique_ptr<ImGuiContext, decltype(&ImGui::DestroyContext)>;
    using Clock = std::chrono::steady_clock;

    Context context_;
    std::vector<Pane> panes_;
    float width_ = kWidth;
    ImTextureID next_texture_id_ = 1;

    void split_tree(ImGuiID node, std::size_t first, std::size_t count, bool horizontal) {
        if (count == 1) {
            Pane& pane = panes_[first];
            pane.node = node;
            for (const std::string& title : pane.titles) {
                ImGui::DockBuilderDockWindow(title.c_str(), node);
            }
            return;
        }
        ImGuiID leading = 0;
        ImGuiID trailing = 0;
        ImGui::DockBuilderSplitNode(node, horizontal ? ImGuiDir_Left : ImGuiDir_Up, 0.5f,
                                   &leading, &trailing);
        require(leading != 0 && trailing != 0 && leading != trailing,
                "DockBuilder did not create two distinct child nodes");
        const std::size_t half = count / 2;
        split_tree(leading, first, half, !horizontal);
        split_tree(trailing, first + half, count - half, !horizontal);
    }

    void build_layout() {
        ImGui::DockBuilderAddNode(kDockspace, ImGuiDockNodeFlags_DockSpace);
        ImGui::DockBuilderSetNodeSize(kDockspace, ImVec2(kWidth, kHeight));
        split_tree(kDockspace, 0, panes_.size(), true);
        ImGui::DockBuilderFinish(kDockspace);
    }

    void acknowledge_textures() {
        ImDrawData* data = ImGui::GetDrawData();
        if (data->Textures == nullptr) {
            return;
        }
        for (ImTextureData* texture : *data->Textures) {
            switch (texture->Status) {
            case ImTextureStatus_WantCreate:
                texture->SetTexID(next_texture_id_++);
                texture->SetStatus(ImTextureStatus_OK);
                break;
            case ImTextureStatus_WantUpdates:
                texture->SetStatus(ImTextureStatus_OK);
                break;
            case ImTextureStatus_WantDestroy:
                texture->SetTexID(ImTextureID_Invalid);
                texture->SetStatus(ImTextureStatus_Destroyed);
                break;
            case ImTextureStatus_OK:
            case ImTextureStatus_Destroyed:
                break;
            }
        }
    }

    Sample run_frame(float width, MouseAction action = MouseAction::None,
                     ImVec2 position = ImVec2(), bool setup = false) {
        std::uint64_t content_calls = 0;
        const auto start = Clock::now();
        ImGuiIO& io = ImGui::GetIO();
        io.DisplaySize = ImVec2(width, kHeight);
        io.DeltaTime = kDeltaTime;
        if (action != MouseAction::None) {
            io.AddMousePosEvent(position.x, position.y);
            if (action == MouseAction::Press || action == MouseAction::Release) {
                io.AddMouseButtonEvent(0, action == MouseAction::Press);
            }
        }
        ImGui::NewFrame();
        if (setup) {
            build_layout();
        }
        ImGui::DockSpaceOverViewport(kDockspace);
        for (Pane& pane : panes_) {
            for (std::size_t tab = 0; tab < 2; ++tab) {
                pane.submitted[tab] = ImGui::Begin(pane.titles[tab].c_str(), nullptr);
                if (pane.submitted[tab]) {
                    ++content_calls;
                    ImGui::TextUnformatted("Content");
                    const ImVec2 cursor = ImGui::GetCursorScreenPos();
                    const ImVec2 square_end(cursor.x + 8.0f, cursor.y + 8.0f);
                    ImGui::GetWindowDrawList()->AddRectFilled(cursor, square_end,
                                                            IM_COL32(80, 160, 220, 255));
                    ImGui::Dummy(ImVec2(8.0f, 8.0f));
                    pane.squares[tab] = ImRect(cursor, square_end);
                }
                ImGui::End();
            }
        }
        ImGui::Render();
        acknowledge_textures();
        const auto end = Clock::now();
        width_ = width;

        // Observe each artifact before another NewFrame invalidates its draw data.
        Sample result;
        result.elapsed_ns = static_cast<std::uint64_t>(
            std::chrono::duration_cast<std::chrono::nanoseconds>(end - start).count());
        result.content_calls = content_calls;
        const ImDrawData* data = ImGui::GetDrawData();
        require(data != nullptr && data->Valid, "Render did not produce valid draw data");
        for (const ImDrawList* list : data->CmdLists) {
            result.vertices += static_cast<std::uint64_t>(list->VtxBuffer.Size);
            result.indices += static_cast<std::uint64_t>(list->IdxBuffer.Size);
            for (const ImDrawCmd& command : list->CmdBuffer) {
                if (command.ElemCount > 0 && command.UserCallback == nullptr) {
                    ++result.draw_commands;
                }
            }
        }
        for (Pane& pane : panes_) {
            for (std::size_t tab = 0; tab < 2; ++tab) {
                if (pane.windows[tab] == nullptr) {
                    pane.windows[tab] = ImGui::FindWindowByName(pane.titles[tab].c_str());
                }
                if (pane.windows[tab] != nullptr && pane.windows[tab]->DockTabIsVisible &&
                    !pane.windows[tab]->Hidden) {
                    ++result.visible_contents;
                }
            }
        }
        return result;
    }

    std::string frame_issue(const Sample& frame) const {
        if (frame.visible_contents != panes_.size()) {
            return "expected " + std::to_string(panes_.size()) + " visible contents, observed " +
                   std::to_string(frame.visible_contents);
        }
        if (frame.content_calls < frame.visible_contents) {
            return "visible contents did not all receive their content callback";
        }
        const ImRect surface(0.0f, 0.0f, width_, kHeight);
        const ImDrawData* data = ImGui::GetDrawData();
        if (data->DisplayPos.x != 0.0f || data->DisplayPos.y != 0.0f ||
            data->DisplaySize.x != width_ || data->DisplaySize.y != kHeight) {
            return "draw data does not match the requested surface bounds";
        }
        for (std::size_t index = 0; index < panes_.size(); ++index) {
            const Pane& pane = panes_[index];
            const ImGuiWindow* first = pane.windows[0];
            const ImGuiWindow* second = pane.windows[1];
            const auto pane_error = [index](const char* message) {
                return "pane " + std::to_string(index + 1) + ": " + message;
            };
            if (first == nullptr || second == nullptr) {
                return pane_error("both windows must exist");
            }
            const ImGuiDockNode* node = first->DockNode;
            if (node == nullptr || node != second->DockNode || node->ID != pane.node ||
                !node->IsLeafNode() || node->Windows.Size != 2 || node->TabBar == nullptr ||
                node->TabBar->Tabs.Size != 2) {
                return pane_error("expected two tabs in the original leaf node");
            }
            if (!node->IsVisible || !has_area(node->Rect()) || !surface.Contains(node->Rect()) ||
                node->Size.x < 64.0f || node->Size.y < 48.0f) {
                return pane_error("the complete pane must remain visible");
            }
            const ImGuiDockNode* root = node;
            while (root->ParentNode != nullptr) {
                root = root->ParentNode;
            }
            if (root->ID != kDockspace || !root->IsDockSpace() || root->Pos.x != 0.0f ||
                root->Pos.y != 0.0f || root->Size.x != width_ || root->Size.y != kHeight) {
                return pane_error("dockspace bounds or root membership changed");
            }
            unsigned visible = 0;
            for (std::size_t tab = 0; tab < 2; ++tab) {
                const ImGuiWindow* window = pane.windows[tab];
                if (!window->Active || !window->DockIsActive || window->DockId != pane.node ||
                    window->HasCloseButton) {
                    return pane_error("window must remain active, docked and without close");
                }
                const ImRect& tab_rect = window->DC.DockTabItemRect;
                if (!has_area(tab_rect) || !surface.Contains(tab_rect) ||
                    !node->TabBar->BarRect.Contains(tab_rect)) {
                    return pane_error("both actual tab rectangles must remain visible");
                }
                const ImGuiTabItem* item = nullptr;
                for (const ImGuiTabItem& candidate : node->TabBar->Tabs) {
                    if (candidate.ID == window->TabId) {
                        item = &candidate;
                    }
                }
                if (item == nullptr || item->Window != window ||
                    item->LastFrameVisible != ImGui::GetFrameCount()) {
                    return pane_error("the actual tab must be submitted this frame");
                }
                if (window->DockTabIsVisible) {
                    ++visible;
                    if (window->Hidden || !pane.submitted[tab] ||
                        node->VisibleWindow != window ||
                        node->TabBar->SelectedTabId != window->TabId ||
                        node->TabBar->VisibleTabId != window->TabId ||
                        node->SelectedTabId != window->TabId) {
                        return pane_error("selected content and visible dock tab disagree");
                    }
                    if (!has_area(pane.squares[tab]) ||
                        !window->InnerClipRect.Contains(pane.squares[tab]) ||
                        window->DrawList->VtxBuffer.Size == 0 ||
                        window->DrawList->IdxBuffer.Size == 0) {
                        return pane_error("selected content must produce visible text and square");
                    }
                }
            }
            if (visible != 1) {
                return pane_error("exactly one selected content must be visible");
            }
        }
        return {};
    }

    void validate_mesh(const Sample& frame) const {
        const ImDrawData* data = ImGui::GetDrawData();
        require(data->FrameCount == ImGui::GetFrameCount(),
                "draw data belongs to another logical frame");
        require(frame.vertices > 0 && frame.indices > 0 && frame.draw_commands > 0,
                "the complete product must produce a positive mesh");
        require(frame.vertices == static_cast<std::uint64_t>(data->TotalVtxCount) &&
                    frame.indices == static_cast<std::uint64_t>(data->TotalIdxCount),
                "draw data totals do not match the complete draw lists");
        for (const ImDrawList* list : data->CmdLists) {
            std::uint64_t command_indices = 0;
            for (const ImDrawCmd& command : list->CmdBuffer) {
                require(command.UserCallback == nullptr,
                        "the workload unexpectedly requires a renderer callback");
                if (command.ElemCount == 0) {
                    continue;
                }
                require(command.GetTexID() != ImTextureID_Invalid,
                        "a mesh command has no acknowledged texture");
                require(static_cast<std::uint64_t>(command.IdxOffset) + command.ElemCount <=
                            static_cast<std::uint64_t>(list->IdxBuffer.Size) &&
                            command.VtxOffset < static_cast<unsigned>(list->VtxBuffer.Size) &&
                            command.ElemCount % 3 == 0,
                        "a mesh command has invalid buffer bounds");
                command_indices += command.ElemCount;
            }
            require(command_indices == static_cast<std::uint64_t>(list->IdxBuffer.Size),
                    "draw commands do not cover the complete index buffer");
        }
        if (data->Textures != nullptr) {
            for (const ImTextureData* texture : *data->Textures) {
                require(texture->Status == ImTextureStatus_OK ||
                            texture->Status == ImTextureStatus_Destroyed,
                        "a texture request was not acknowledged");
            }
        }
    }

    Sample checked_frame(float width, MouseAction action = MouseAction::None,
                         ImVec2 position = ImVec2()) {
        Sample result = run_frame(width, action, position);
        const std::string issue = frame_issue(result);
        if (!issue.empty()) {
            fail("panes=" + std::to_string(panes_.size()) + ": " + issue);
        }
        validate_mesh(result);
        return result;
    }

    Sample click_inactive_tab() {
        // Both products observe geometry, then complete an untimed mouse-hover pass.
        checked_frame(width_);
        std::vector<ImGuiID> selected;
        selected.reserve(panes_.size());
        for (const Pane& pane : panes_) {
            selected.push_back(pane.windows[0]->DockNode->TabBar->SelectedTabId);
        }
        const Pane& pane = panes_.front();
        const ImGuiWindow* target =
            pane.windows[0]->TabId == selected.front() ? pane.windows[1] : pane.windows[0];
        const ImRect tab_rect = target->DC.DockTabItemRect;
        const ImVec2 position = tab_rect.GetCenter();
        require(has_area(tab_rect) && tab_rect.Contains(position) &&
                    pane.windows[0]->DockNode->TabBar->BarRect.Contains(position) &&
                    target->TabId != selected.front(),
                "the click must hit the actual inactive tab in the first pane");

        checked_frame(width_, MouseAction::Move, position);
        const ImRect hovered_tab_rect = target->DC.DockTabItemRect;
        require(has_area(hovered_tab_rect) && hovered_tab_rect.Contains(position) &&
                    pane.windows[0]->DockNode->TabBar->BarRect.Contains(position),
                "the actual tab moved away from the prepared hover position");
        require(!ImGui::GetIO().MouseDown[0],
                "the hover preparation must not press the mouse button");
        require(ImGui::GetCurrentContext()->HoveredId == target->TabId,
                "the hover preparation did not observe the inactive tab");
        for (std::size_t index = 0; index < panes_.size(); ++index) {
            if (panes_[index].windows[0]->DockNode->TabBar->SelectedTabId != selected[index]) {
                fail("the hover preparation changed the selected tab in pane " +
                     std::to_string(index + 1));
            }
        }

        Sample result = checked_frame(width_, MouseAction::Press, position);
        require(ImGui::GetIO().MouseDown[0], "the press event was not consumed");
        const ImGuiContext& context = *ImGui::GetCurrentContext();
        const ImGuiID press_selected = pane.windows[0]->DockNode->TabBar->SelectedTabId;
        const ImGuiID press_pending = pane.windows[0]->DockNode->TabBar->NextSelectedTabId;
        const ImGuiID press_hovered = context.HoveredId;
        const ImGuiID press_previous_hovered = context.HoveredIdPreviousFrame;
        const ImGuiWindow* press_hovered_window = context.HoveredWindow;
        const int press_queued_events = context.InputEventsQueue.Size;
        const Sample released = checked_frame(width_, MouseAction::Release, position);
        require(!ImGui::GetIO().MouseDown[0], "the release event was not consumed");
        result.add(released);

        for (std::size_t index = 0; index < panes_.size(); ++index) {
            const ImGuiDockNode* node = panes_[index].windows[0]->DockNode;
            const ImGuiID expected = index == 0 ? target->TabId : selected[index];
            if (node->TabBar->SelectedTabId != expected || node->VisibleWindow == nullptr ||
                node->VisibleWindow->TabId != expected) {
                const ImGuiIO& io = ImGui::GetIO();
                const ImRect& current_rect = target->DC.DockTabItemRect;
                std::ostringstream message;
                message << "tab_click selection mismatch: panes=" << panes_.size()
                        << " pane=" << index + 1 << " frame=" << ImGui::GetFrameCount()
                        << std::hex << " expected=0x" << expected
                        << " actual=0x" << node->TabBar->SelectedTabId << " visible=0x"
                        << (node->VisibleWindow ? node->VisibleWindow->TabId : 0)
                        << " target=0x" << target->TabId << " target_name='" << target->Name
                        << "' before=0x" << selected[index]
                        << " press_selected=0x" << press_selected
                        << " press_pending=0x" << press_pending
                        << " press_hovered=0x" << press_hovered
                        << " press_previous_hovered=0x" << press_previous_hovered
                        << " release_pending=0x" << node->TabBar->NextSelectedTabId
                        << " release_hovered=0x" << context.HoveredId
                        << " release_previous_hovered=0x" << context.HoveredIdPreviousFrame
                        << std::dec << " press_queued=" << press_queued_events
                        << " release_queued=" << context.InputEventsQueue.Size
                        << " trickle=" << io.ConfigInputTrickleEventQueue
                        << " mouse_down=" << io.MouseDown[0]
                        << " mouse_released=" << io.MouseReleased[0]
                        << " press_hovered_window='"
                        << (press_hovered_window ? press_hovered_window->Name : "<none>")
                        << "' release_hovered_window='"
                        << (context.HoveredWindow ? context.HoveredWindow->Name : "<none>")
                        << "' mouse=(" << io.MousePos.x << ',' << io.MousePos.y
                        << ") click=(" << position.x << ',' << position.y << ") tab_before=("
                        << tab_rect.Min.x << ',' << tab_rect.Min.y << ',' << tab_rect.Max.x << ','
                        << tab_rect.Max.y << ") tab_after=(" << current_rect.Min.x << ','
                        << current_rect.Min.y << ',' << current_rect.Max.x << ','
                        << current_rect.Max.y << ')';
                fail(message.str());
            }
        }
        return result;
    }
};

void write_sample(std::size_t panes, Scenario scenario, std::size_t index, const Sample& sample) {
    std::cout << "dear_imgui," << panes << ",2," << scenario_name(scenario) << ',' << index << ','
              << sample.frames << ',' << sample.elapsed_ns << ',' << sample.passes << ','
              << sample.content_calls << ',' << sample.visible_contents << ',' << sample.vertices
              << ',' << sample.indices << ',' << sample.draw_commands << '\n';
}

} // namespace

int main(int argc, char** argv) {
    try {
        const Config config = parse_args(argc, argv);
        if (config.help) {
            std::cerr << "Usage: imgui_product_cost [--samples 24] [--warmup 6] "
                         "[--panes 16,128,1024]\n";
            return 0;
        }
        std::cout << "product,panes,tabs_per_pane,scenario,sample,frames,elapsed_ns,passes,"
                     "content_calls,visible_contents,vertices,indices,draw_commands\n";
        for (std::size_t panes : config.panes) {
            for (Scenario scenario :
                 {Scenario::Idle, Scenario::TabClick, Scenario::SurfaceResize}) {
                ImGuiProduct product(panes);
                for (std::size_t warmup = 0; warmup < config.warmup; ++warmup) {
                    product.sample(scenario);
                }
                for (std::size_t sample = 0; sample < config.samples; ++sample) {
                    write_sample(panes, scenario, sample, product.sample(scenario));
                }
            }
        }
        std::cout.flush();
        require(static_cast<bool>(std::cout), "failed to write the complete CSV output");
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "dear_imgui product benchmark: " << error.what() << '\n';
        return 1;
    }
}
