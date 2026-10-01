// Toroidal void-and-cluster blue-noise threshold generator.
//
// Method: R. A. Ulichney, "The Void-and-Cluster Method for Generating Dither
// Arrays," Proc. SPIE vol. 1913, pp. 332-343 (1993). Self-contained
// three-phase pipeline: homogenize a ~10% random pattern by
// swap-convergence and keep the resulting prototype (never mutated
// afterwards); phase I strips densest clusters (ranks n-1..0); phase II
// refills largest voids from a fresh prototype clone (ranks n..half-1);
// phase III orders the complementary white minority (ranks half..N-1).
// Convergence holds iff removing the 1 from the tightest cluster creates
// the largest void at the same cell, at which point the removed dot is
// restored and the phase ends.
//
// The prototype's per-pixel FFT recompute is O(N^2) per step and unsuitable
// for full-screen grids, so this generator keeps an incremental filtered
// field instead: toggling a dot adds/subtracts a finite wrapped Gaussian
// (sigma 1.5, support radius R, default 7, i.e. a 15x15 stencil) to the
// toroidal neighbourhood. Densest-cluster / largest-void queries are served
// by lazy binary heaps (max over ON cells, min over OFF cells) keyed by the
// cached field with version stamps; stale entries are discarded on pop.
// Tie-breaks come from a seeded per-cell hash, so the run is fully
// deterministic for a fixed 64-bit seed.
//
// Build (host only, binary is untracked build output):
//   c++ -O3 -std=c++17 -o generator/generate generator/generate.cpp
// Run:
//   ./generator/generate --width 600 --height 600 --seed 600600 \
//       --output assets/blue-noise-600x600.bin
// Warmup fixture (32x32, checks the incremental cache against a full
// wrapped-Gaussian recompute, plus permutation/seed/stability):
//   ./generator/generate --self-check
//
// No absolute paths are hardcoded: --output and --progress take the caller's
// paths as arguments.

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <ctime>
#include <fstream>
#include <string>
#include <vector>
#include <chrono>

// ---------------------------------------------------------------- RNG ---

static uint64_t splitmix64(uint64_t &s) {
    uint64_t z = (s += 0x9E3779B97F4A7C15ULL);
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
    z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
    return z ^ (z >> 31);
}

static uint64_t hash64(uint64_t x) {
    x ^= x >> 30; x *= 0xBF58476D1CE4E5B9ULL;
    x ^= x >> 27; x *= 0x94D049BB133111EBULL;
    return x ^ (x >> 31);
}

// ------------------------------------------------------------- options ---

struct Opts {
    int W = 600, H = 300;
    uint64_t seed = 400300;
    double sigma = 1.5;
    int radius = 7;          // finite kernel support; 6..8 documented range
    double density = 0.10;   // initial black density for homogenization
    long maxSwaps = 2000000; // homogenization cap; failure aborts, no partial
    std::string output;
    std::string progress;    // atomic ledger path; empty disables
    long progressEvery = 8192;
    bool selfCheck = false;
};

static bool parseArgs(int argc, char **argv, Opts &o, std::string &err) {
    for (int i = 1; i < argc; i++) {
        std::string a = argv[i];
        auto need = [&](const char *k, std::string &dst) {
            if (++i >= argc) { err = std::string("missing value for ") + k; return false; }
            dst = argv[i]; return true;
        };
        if (a == "--width") { std::string v; if (!need("W", v)) return false; o.W = std::stoi(v); }
        else if (a == "--height") { std::string v; if (!need("H", v)) return false; o.H = std::stoi(v); }
        else if (a == "--seed") { std::string v; if (!need("seed", v)) return false; o.seed = std::stoull(v, nullptr, 0); }
        else if (a == "--sigma") { std::string v; if (!need("sigma", v)) return false; o.sigma = std::stod(v); }
        else if (a == "--radius") { std::string v; if (!need("radius", v)) return false; o.radius = std::stoi(v); }
        else if (a == "--density") { std::string v; if (!need("density", v)) return false; o.density = std::stod(v); }
        else if (a == "--max-swaps") { std::string v; if (!need("maxSwaps", v)) return false; o.maxSwaps = std::stol(v); }
        else if (a == "--output") { if (!need("--output", o.output)) return false; }
        else if (a == "--progress") { if (!need("--progress", o.progress)) return false; }
        else if (a == "--progress-every") { std::string v; if (!need("progressEvery", v)) return false; o.progressEvery = std::stol(v); }
        else if (a == "--self-check") { o.selfCheck = true; }
        else { err = "unknown flag " + a; return false; }
    }
    if (!o.selfCheck && o.output.empty()) { err = "--output required"; return false; }
    if (o.radius < 6 || o.radius > 8) { err = "radius outside documented 6..8 range"; return false; }
    return true;
}

// ----------------------------------------------------------- generator ---

struct Gen {
    int W, H, N, R;
    double sigma;
    uint64_t seed;
    std::vector<float> k;        // (2R+1)^2 finite Gaussian stencil, peak 1
    std::vector<float> d;        // incremental filtered field
    std::vector<char> on;
    std::vector<int> ver;        // heap version stamps
    std::vector<int> cnt;        // ON count (scalar kept by caller pattern)
    std::vector<uint64_t> tie;   // seeded per-cell tie-break keys
    int curVer = 0;
    int onCount = 0;

    struct Node { float v; uint64_t t; int idx; int ver; };
    struct MaxCmp { bool operator()(const Node &a, const Node &b) const {
        if (a.v != b.v) return a.v < b.v; return a.t > b.t; } };
    struct MinCmp { bool operator()(const Node &a, const Node &b) const {
        if (a.v != b.v) return a.v > b.v; return a.t > b.t; } };
    std::vector<Node> heapMax, heapMin; // binary heaps with lazy deletion

    Gen(int w, int h, double sig, int r, uint64_t s)
        : W(w), H(h), N(w * h), R(r), sigma(sig), seed(s) {
        k.resize((size_t)(2 * R + 1) * (2 * R + 1));
        for (int dy = -R; dy <= R; dy++)
            for (int dx = -R; dx <= R; dx++) {
                double g = std::exp(-(dx * dx + dy * dy) / (2 * sigma * sigma));
                k[(size_t)(dy + R) * (2 * R + 1) + (dx + R)] = (float)g;
            }
        d.assign(N, 0.0f);
        on.assign(N, 0);
        ver.assign(N, 0);
        tie.resize(N);
        for (int i = 0; i < N; i++)
            tie[i] = hash64(seed ^ 0x9E3779B97F4A7C15ULL ^ (uint64_t)(uint32_t)i);
        heapMax.reserve(4 * 1024 * 1024);
        heapMin.reserve(4 * 1024 * 1024);
    }

    inline int wrapX(int x) const { int m = x % W; return m < 0 ? m + W : m; }
    inline int wrapY(int y) const { int m = y % H; return m < 0 ? m + H : m; }

    static void push(std::vector<Node> &h, Node n, bool isMax) {
        h.push_back(n);
        size_t i = h.size() - 1;
        while (i > 0) {
            size_t p = (i - 1) / 2;
            bool swapUp = isMax
                ? (h[i].v > h[p].v || (h[i].v == h[p].v && h[i].t < h[p].t))
                : (h[i].v < h[p].v || (h[i].v == h[p].v && h[i].t < h[p].t));
            if (!swapUp) break;
            Node t = h[i]; h[i] = h[p]; h[p] = t; i = p;
        }
    }

    static Node popRaw(std::vector<Node> &h, bool isMax) {
        Node top = h[0];
        h[0] = h.back(); h.pop_back();
        size_t i = 0;
        for (;;) {
            size_t l = 2 * i + 1, r = l + 1, b = i;
            auto better = [&](size_t a, size_t c) {
                return isMax
                    ? (h[a].v > h[c].v || (h[a].v == h[c].v && h[a].t < h[c].t))
                    : (h[a].v < h[c].v || (h[a].v == h[c].v && h[a].t < h[c].t));
            };
            if (l < h.size() && better(l, b)) b = l;
            if (r < h.size() && better(r, b)) b = r;
            if (b == i) break;
            Node t = h[i]; h[i] = h[b]; h[b] = t; i = b;
        }
        return top;
    }

    void pushCell(int idx) {
        curVer++;
        ver[idx] = curVer;
        Node n{d[idx], tie[idx], idx, curVer};
        if (on[idx]) push(heapMax, n, true);
        else push(heapMin, n, false);
    }

    int popMaxOn() {
        for (;;) {
            if (heapMax.empty()) { std::fprintf(stderr, "heap underflow (max)\n"); std::exit(1); }
            Node n = popRaw(heapMax, true);
            if (n.ver == ver[n.idx] && on[n.idx]) return n.idx;
        }
    }

    int popMinOff() {
        for (;;) {
            if (heapMin.empty()) { std::fprintf(stderr, "heap underflow (min)\n"); std::exit(1); }
            Node n = popRaw(heapMin, false);
            if (n.ver == ver[n.idx] && !on[n.idx]) return n.idx;
        }
    }

    // Toggle one dot and incrementally update the toroidal neighbourhood.
    void setDot(int idx, bool v) {
        if (on[idx] == (char)v) return;
        float s = v ? 1.0f : -1.0f;
        int cx = idx % W, cy = idx / W;
        for (int dy = -R; dy <= R; dy++) {
            int y = wrapY(cy + dy);
            for (int dx = -R; dx <= R; dx++) {
                int x = wrapX(cx + dx);
                d[(size_t)y * W + x] += s * k[(size_t)(dy + R) * (2 * R + 1) + (dx + R)];
            }
        }
        on[idx] = (char)v;
        onCount += v ? 1 : -1;
        pushCell(idx);
        // Neighbours' cached keys changed; re-push them (lazy: old entries
        // go stale via the version stamp).
        for (int dy = -R; dy <= R; dy++) {
            int y = wrapY(cy + dy);
            for (int dx = -R; dx <= R; dx++) {
                if (!dx && !dy) continue;
                int x = wrapX(cx + dx);
                pushCell(y * W + x);
            }
        }
    }

    // Rebuild the whole field from the pattern (phase resets / validation).
    void rebuild(const std::vector<char> &pat) {
        std::fill(d.begin(), d.end(), 0.0f);
        on = pat;
        onCount = 0;
        for (int i = 0; i < N; i++) if (on[i]) onCount++;
        heapMax.clear(); heapMin.clear();
        // Seed heaps by direct accumulation: add each dot's stencil once.
        for (int i = 0; i < N; i++) {
            if (!on[i]) continue;
            int cx = i % W, cy = i / W;
            for (int dy = -R; dy <= R; dy++) {
                int y = wrapY(cy + dy);
                for (int dx = -R; dx <= R; dx++) {
                    int x = wrapX(cx + dx);
                    d[(size_t)y * W + x] += k[(size_t)(dy + R) * (2 * R + 1) + (dx + R)];
                }
            }
        }
        for (int i = 0; i < N; i++) pushCell(i);
    }

    // Full wrapped-Gaussian recompute (independent check of the cache).
    // Uses the full-grid wrapped kernel like the 32x32 prototype.
    std::vector<float> recomputeFull(const std::vector<char> &pat) const {
        std::vector<float> f(N, 0.0f);
        std::vector<float> kern(N);
        for (int y = 0; y < H; y++)
            for (int x = 0; x < W; x++) {
                int dx = x < W - x ? x : W - x;
                int dy = y < H - y ? y : H - y;
                kern[(size_t)y * W + x] =
                    (float)std::exp(-(dx * dx + dy * dy) / (2 * sigma * sigma));
            }
        for (int oy = 0; oy < H; oy++)
            for (int ox = 0; ox < W; ox++) {
                if (!pat[(size_t)oy * W + ox]) continue;
                for (int y = 0; y < H; y++)
                    for (int x = 0; x < W; x++) {
                        int kx = x - ox; if (kx < 0) kx += W;
                        int ky = y - oy; if (ky < 0) ky += H;
                        f[(size_t)y * W + x] += kern[(size_t)ky * W + kx];
                    }
            }
        return f;
    }

    float cacheError(const std::vector<char> &pat) {
        std::vector<float> f = recomputeFull(pat);
        float peak = 0, err = 0;
        for (int i = 0; i < N; i++) {
            peak = std::max(peak, std::abs(f[i]));
            err = std::max(err, std::abs(f[i] - d[i]));
        }
        return peak > 0 ? err / peak : err;
    }
};

// ------------------------------------------------------------- driver ---

static std::string now() {
    char b[32];
    std::time_t t = std::time(nullptr);
    std::strftime(b, sizeof b, "%H:%M:%S", std::localtime(&t));
    return b;
}

// Atomic ledger write: tmp file + rename, one compact line per update.
static void ledger(const std::string &path, const std::string &line) {
    if (path.empty()) return;
    std::string tmp = path + ".tmp";
    { std::ofstream f(tmp, std::ios::trunc); f << line << "\n"; }
    std::rename(tmp.c_str(), path.c_str());
}

static void report(Gen &g, const Opts &o, const char *phase, long done, long total,
                   const std::chrono::steady_clock::time_point &t0) {
    using namespace std::chrono;
    double el = duration_cast<milliseconds>(steady_clock::now() - t0).count() / 1000.0;
    std::fprintf(stderr, "[%s] %dx%d %s %ld/%ld (%.1f%%) %.0fs\n",
                 now().c_str(), g.W, g.H, phase, done, total,
                 total ? 100.0 * done / total : 0.0, el);
    if (!o.progress.empty() && (done % o.progressEvery == 0 || done == total)) {
        char line[512];
        std::snprintf(line, sizeof line,
                      "ts=%s grid=%dx%d seed=%llu phase=%s count=%ld/%ld elapsed_s=%.0f",
                      now().c_str(), g.W, g.H, (unsigned long long)o.seed,
                      phase, done, total, el);
        ledger(o.progress, line);
    }
}

// Homogenize per the prototype: densest dot out, field recomputed
// incrementally (equivalent: the void comes from the post-removal field),
// largest void in; converge when the void is the removed cell (restored).
static std::vector<char> homogenize(Gen &g, Opts &o,
                                    const std::chrono::steady_clock::time_point &t0) {
    uint64_t s = o.seed ^ 0x51EDULL;
    std::vector<char> pat(g.N, 0);
    for (int i = 0; i < g.N; i++) {
        uint64_t r = splitmix64(s);
        pat[i] = (r < (uint64_t)(o.density * 18446744073709551616.0)) ? 1 : 0;
    }
    if (g.N > 0 && !std::any_of(pat.begin(), pat.end(), [](char c){return c; }))
        pat[0] = 1;
    g.rebuild(pat);
    for (long it = 0; it < o.maxSwaps; it++) {
        int ci = g.popMaxOn();
        g.setDot(ci, false);          // removal updates the cached field
        int vi = g.popMinOff();       // void from the post-removal field
        if (vi == ci) { g.setDot(vi, true); return g.on; }
        g.setDot(vi, true);
        if ((it + 1) % 4096 == 0)
            report(g, o, "homogenize", it + 1, o.maxSwaps, t0);
    }
    std::fprintf(stderr, "homogenization did not converge in %ld swaps\n", o.maxSwaps);
    std::exit(1);
}

static std::vector<int32_t> order(Gen &g, Opts &o,
                                  const std::chrono::steady_clock::time_point &t0) {
    std::vector<char> proto = homogenize(g, o, t0);
    int nBlack = 0;
    for (char c : proto) nBlack += c;
    int half = g.N / 2;
    std::vector<int32_t> rank(g.N, -1);

    // Phase I: clone the prototype, strip densest clusters -> nBlack-1..0.
    g.rebuild(proto);
    long total1 = nBlack, done = 0;
    while (g.onCount > 0) {
        int ci = g.popMaxOn();
        if (rank[ci] != -1) { std::fprintf(stderr, "phase I overwrite\n"); std::exit(1); }
        rank[ci] = g.onCount - 1;
        g.setDot(ci, false);
        if (++done % o.progressEvery == 0) report(g, o, "phase-I", done, total1, t0);
    }
    report(g, o, "phase-I", done, total1, t0);

    // Phase II: RESET to a fresh prototype clone, fill voids -> nBlack..half-1.
    g.rebuild(proto);
    long total2 = half - nBlack; done = 0;
    while (g.onCount < half) {
        int vi = g.popMinOff();
        if (rank[vi] != -1) { std::fprintf(stderr, "phase II overwrite\n"); std::exit(1); }
        rank[vi] = g.onCount;
        g.setDot(vi, true);
        if (++done % o.progressEvery == 0) report(g, o, "phase-II", done, total2, t0);
    }
    report(g, o, "phase-II", done, total2, t0);

    // Phase III: white minority = inverse of the half-filled pattern.
    std::vector<char> white(g.N);
    for (int i = 0; i < g.N; i++) white[i] = g.on[i] ? 0 : 1;
    g.rebuild(white);
    long total3 = g.N - half; done = 0;
    int cursor = half;
    while (g.onCount > 0) {
        int ci = g.popMaxOn();
        if (rank[ci] != -1) { std::fprintf(stderr, "phase III overwrite\n"); std::exit(1); }
        rank[ci] = cursor++;
        g.setDot(ci, false);
        if (++done % o.progressEvery == 0) report(g, o, "phase-III", done, total3, t0);
    }
    report(g, o, "phase-III", done, total3, t0);

    // Permutation + prototype-preservation checks (no silent partials).
    std::vector<char> seen(g.N, 0);
    for (int32_t r : rank) {
        if (r < 0 || r >= g.N || seen[r]) { std::fprintf(stderr, "rank not a permutation\n"); std::exit(1); }
        seen[r] = 1;
    }
    for (int i = 0; i < g.N; i++) {
        bool inProto = proto[i] != 0;
        bool inLow = rank[i] < nBlack;
        if (inProto != inLow) { std::fprintf(stderr, "prototype not rank set 0..n-1\n"); std::exit(1); }
    }
    return rank;
}

// ---------------------------------------------------------- self-check ---

static int selfCheck() {
    const int N = 32, R = 7;
    const double SIG = 1.5;
    const uint64_t SEED = 0xC10C4ULL;
    std::fprintf(stderr, "self-check: 32x32 warmup fixture\n");

    // 1. Impulse response: peak at origin, wrap-symmetric.
    {
        Gen g(N, N, SIG, R, SEED);
        std::vector<char> pat(N * N, 0);
        pat[0] = 1;
        g.rebuild(pat);
        float peak = g.d[0];
        for (float v : g.d)
            if (v > peak + 1e-6f) { std::fprintf(stderr, "impulse peak not at origin\n"); return 1; }
        auto at = [&](int x, int y) { return g.d[(size_t)y * N + x]; };
        if (std::abs(at(1, 0) - at(N - 1, 0)) > 1e-6f ||
            std::abs(at(0, 1) - at(0, N - 1)) > 1e-6f ||
            std::abs(at(1, 1) - at(N - 1, N - 1)) > 1e-6f) {
            std::fprintf(stderr, "impulse wrap asymmetry\n"); return 1;
        }
        std::fprintf(stderr, "  impulse ok (peak %.6f)\n", peak);
    }

    // 2. Random toggles: incremental cache vs full wrapped recompute.
    {
        Gen g(N, N, SIG, R, SEED);
        std::vector<char> pat(N * N, 0);
        g.rebuild(pat);
        uint64_t rng = SEED ^ 0x7073ULL;
        for (int k = 0; k < 2000; k++) {
            int idx = (int)(splitmix64(rng) % (uint64_t)(N * N));
            g.setDot(idx, !g.on[idx]);
        }
        float err = g.cacheError(g.on);
        std::fprintf(stderr, "  random-toggle cache rel-err %.2e\n", err);
        if (err > 2e-4f) { std::fprintf(stderr, "cache diverged from recompute\n"); return 1; }
    }

    // 3-6. Full 32x32 ordering: permutation/hist/seed/stability.
    auto run32 = [&](uint64_t seed) {
        Opts o;
        o.W = N; o.H = N; o.seed = seed; o.sigma = SIG; o.radius = R;
        Gen g(N, N, SIG, R, seed);
        auto t0 = std::chrono::steady_clock::now();
        return order(g, o, t0);
    };
    std::vector<int32_t> r1 = run32(SEED);
    {
        std::vector<char> seen(N * N, 0);
        for (int32_t r : r1) seen[r] = 1;
        for (char c : seen) if (!c) { std::fprintf(stderr, "32 ranks not permutation\n"); return 1; }
        int hist[256] = {0};
        for (int32_t r : r1) hist[(r >> 2)]++;
        for (int c : hist) if (c != 4) { std::fprintf(stderr, "32 hist not flat\n"); return 1; }
        std::fprintf(stderr, "  ranks permutation + flat hist ok\n");
    }
    std::vector<int32_t> r2 = run32(SEED ^ 0x1ULL);
    {
        int diff = 0;
        for (int i = 0; i < N * N; i++) diff += (r1[i] != r2[i]);
        std::fprintf(stderr, "  seed sensitivity: %d/%d differ\n", diff, N * N);
        if (!diff) { std::fprintf(stderr, "seed change left output identical\n"); return 1; }
    }
    {
        std::vector<int32_t> r3 = run32(SEED);
        if (r3 != r1) { std::fprintf(stderr, "same seed gave different output\n"); return 1; }
        std::fprintf(stderr, "  stable output ok\n");
    }
    std::fprintf(stderr, "self-check passed\n");
    return 0;
}

// ---------------------------------------------------------------- main ---

int main(int argc, char **argv) {
    Opts o;
    std::string err;
    if (!parseArgs(argc, argv, o, err)) {
        std::fprintf(stderr, "usage error: %s\n", err.c_str());
        return 2;
    }
    if (o.selfCheck) return selfCheck();

    auto t0 = std::chrono::steady_clock::now();
    std::fprintf(stderr, "[%s] start %dx%d seed=%llu sigma=%.2f radius=%d density=%.2f\n",
                 now().c_str(), o.W, o.H, (unsigned long long)o.seed,
                 o.sigma, o.radius, o.density);
    ledger(o.progress, "ts=" + now() + " grid=" + std::to_string(o.W) + "x" +
           std::to_string(o.H) + " phase=start");
    Gen g(o.W, o.H, o.sigma, o.radius, o.seed);
    std::vector<int32_t> rank = order(g, o, t0);

    std::vector<uint8_t> out(g.N);
    for (int i = 0; i < g.N; i++)
        out[i] = (uint8_t)((int64_t)rank[i] * 256 / g.N);
    {
        std::string tmp = o.output + ".tmp";
        std::ofstream f(tmp, std::ios::binary | std::ios::trunc);
        if (!f) { std::fprintf(stderr, "cannot open output\n"); return 1; }
        f.write((const char *)out.data(), (std::streamsize)out.size());
        f.close();
        if (!f) { std::fprintf(stderr, "output write failed\n"); return 1; }
        std::rename(tmp.c_str(), o.output.c_str());
    }
    using namespace std::chrono;
    double el = duration_cast<milliseconds>(steady_clock::now() - t0).count() / 1000.0;
    std::fprintf(stderr, "[%s] wrote %s (%d bytes) in %.0fs\n",
                 now().c_str(), o.output.c_str(), g.N, el);
    char line[512];
    std::snprintf(line, sizeof line, "ts=%s grid=%dx%d seed=%llu phase=done elapsed_s=%.0f artifact=ready",
                  now().c_str(), o.W, o.H, (unsigned long long)o.seed, el);
    ledger(o.progress, line);
    return 0;
}
