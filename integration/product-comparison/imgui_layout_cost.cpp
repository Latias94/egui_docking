// Compile the pinned, unmodified upstream implementation in this translation unit
// to call its private layout routine without introducing an upstream patch.
#include "imgui.cpp"

#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <iomanip>
#include <iostream>
#include <memory>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
constexpr float kHeight = 16380.0f;
constexpr float kWidths[] = {16380.0f, 16444.0f};
constexpr float kSeparator = 4.0f;
using Clock = std::chrono::steady_clock;

void require(bool condition, const char* message) {
    if (!condition)
        throw std::runtime_error(message);
}

int setting(const char* name, int fallback) {
    const char* value = std::getenv(name);
    if (!value)
        return fallback;
    std::size_t consumed = 0;
    const int parsed = std::stoi(value, &consumed);
    require(consumed == std::string(value).size() && parsed >= 0, "invalid setting");
    return parsed;
}

ImGuiDockNode* tree(std::vector<std::unique_ptr<ImGuiDockNode>>& nodes, int count, ImGuiAxis axis) {
    auto node = std::make_unique<ImGuiDockNode>(static_cast<ImGuiID>(nodes.size() + 1));
    ImGuiDockNode* result = node.get();
    result->SizeRef = ImVec2(1.0f, 1.0f);
    nodes.push_back(std::move(node));
    if (count > 1) {
        result->SplitAxis = axis;
        const ImGuiAxis next = axis == ImGuiAxis_X ? ImGuiAxis_Y : ImGuiAxis_X;
        result->ChildNodes[0] = tree(nodes, count / 2, next);
        result->ChildNodes[1] = tree(nodes, count / 2, next);
        for (ImGuiDockNode* child : result->ChildNodes)
            child->ParentNode = result;
    }
    return result;
}

void expected_rects(int count, ImVec2 pos, ImVec2 size, ImGuiAxis axis, std::vector<ImRect>& rows) {
    rows.emplace_back(pos, ImVec2(pos.x + size.x, pos.y + size.y));
    if (count == 1)
        return;
    ImVec2 child_size = size;
    child_size[axis] = (size[axis] - kSeparator) * 0.5f;
    ImVec2 second = pos;
    second[axis] += child_size[axis] + kSeparator;
    const ImGuiAxis next = axis == ImGuiAxis_X ? ImGuiAxis_Y : ImGuiAxis_X;
    expected_rects(count / 2, pos, child_size, next, rows);
    expected_rects(count / 2, second, child_size, next, rows);
}

std::uint64_t validate(const std::vector<std::unique_ptr<ImGuiDockNode>>& nodes,
                       const std::vector<ImRect>& expected, int leaves) {
    require(nodes.size() == expected.size(), "wrong node count");
    std::uint64_t digest = UINT64_C(0xcbf29ce484222325);
    int observed_leaves = 0;
    for (std::size_t i = 0; i < nodes.size(); ++i) {
        const ImGuiDockNode& node = *nodes[i];
        const ImRect& rect = expected[i];
        require(node.Pos.x == rect.Min.x && node.Pos.y == rect.Min.y &&
                    node.Size.x == rect.GetWidth() && node.Size.y == rect.GetHeight(),
                "layout geometry differs from the complete common-subset oracle");
        observed_leaves += node.IsLeafNode() ? 1 : 0;
        for (const float value : {node.Pos.x, node.Pos.y, node.Size.x, node.Size.y}) {
            require(std::isfinite(value) && value >= 0 && value <= 65536 && std::trunc(value) == value,
                    "geometry must be an exact nonnegative integer");
            const std::uint64_t integer = static_cast<std::uint64_t>(value);
            for (int byte = 0; byte < 8; ++byte)
                digest = (digest ^ ((integer >> (byte * 8)) & 255)) * UINT64_C(0x100000001b3);
        }
    }
    require(observed_leaves == leaves, "missing visible leaf");
    return digest;
}

struct Context {
    ImGuiContext* value = ImGui::CreateContext();
    Context() {
        ImGui::GetIO().IniFilename = nullptr;
        ImGui::GetStyle().DockingSeparatorSize = kSeparator;
        ImGui::GetStyle().WindowMinSize = ImVec2(0.0f, 0.0f);
    }
    ~Context() { ImGui::DestroyContext(value); }
};
} // namespace

int main() {
    try {
        const int samples = setting("DOCKSPACE_LAYOUT_COST_SAMPLES", 96);
        const int warmup = setting("DOCKSPACE_LAYOUT_COST_WARMUP", 16);
        require(samples > 0 && warmup <= 1000000 && samples <= 1000000, "invalid sample count");
        const char* configured = std::getenv("DOCKSPACE_LAYOUT_COST_PANES");
        std::istringstream panes(configured ? configured : "16,128,1024");
        Context context;
        std::cout << "product,panes,case,sample,preparation_ns,elapsed_ns,nodes,leaves,splits,width,height,geometry_digest\n";
        std::string part;
        while (std::getline(panes, part, ',')) {
            const int count = std::stoi(part);
            require(count == 16 || count == 128 || count == 1024, "unsupported pane count");
            std::vector<std::unique_ptr<ImGuiDockNode>> nodes;
            nodes.reserve(static_cast<std::size_t>(count * 2 - 1));
            ImGuiDockNode* root = tree(nodes, count, ImGuiAxis_X);
            std::vector<ImRect> expected[2];
            for (int variant = 0; variant < 2; ++variant)
                expected_rects(count, ImVec2(0.0f, 0.0f), ImVec2(kWidths[variant], kHeight), ImGuiAxis_X, expected[variant]);
            for (int iteration = 0; iteration < warmup + samples; ++iteration) {
                const int variant = iteration % 2;
                const auto start = Clock::now();
                std::atomic_signal_fence(std::memory_order_seq_cst);
                ImGui::DockNodeTreeUpdatePosSize(root, ImVec2(0.0f, 0.0f), ImVec2(kWidths[variant], kHeight));
                std::atomic_signal_fence(std::memory_order_seq_cst);
                const auto end = Clock::now();
                const std::uint64_t digest = validate(nodes, expected[variant], count);
                if (iteration >= warmup) {
                    const auto elapsed = std::chrono::duration_cast<std::chrono::nanoseconds>(end - start).count();
                    std::cout << "dear_imgui," << count << ",project_in_place," << iteration - warmup
                              << ",0," << elapsed << ',' << nodes.size() << ',' << count << ',' << count - 1
                              << ',' << kWidths[variant] << ',' << kHeight << ','
                              << std::hex << std::setw(16) << std::setfill('0') << digest << std::dec << '\n';
                }
            }
        }
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
