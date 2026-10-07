// gauntlet_acceptance.hpp -- tiny, dependency-free runtime for Gherkin acceptance tests (C++17).
//
// Step definitions (in acceptance/steps/*.cpp):
//
//   #include "gauntlet_acceptance.hpp"
//   GT_STEP("a stack with {int} items") {
//     auto& s = ctx.get<Stack>("stack");          // per-scenario state
//     for (int i = 0; i < args.i(0); ++i) s.push(i);
//   }
//   GT_STEP("the top is {int}") { GT_EXPECT_EQ(ctx.get<Stack>("stack").top(), args.i(0)); }
//
// Parameter types: {int} {float} {word} {string} (double-quoted) {} (anything); "(s)" = optional text.
// A pattern written as "^...$" is used as a raw ECMAScript regex.
#pragma once

#include <any>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <fstream>
#include <functional>
#include <map>
#include <memory>
#include <regex>
#include <sstream>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace gauntlet {

struct StepFailure : std::runtime_error {
  using std::runtime_error::runtime_error;
};

using Table = std::vector<std::vector<std::string>>;

struct StepArgs {
  std::vector<std::string> captures;
  std::string doc_string;
  Table table;

  const std::string& s(size_t n) const { return at(n); }
  long long i(size_t n) const { return std::stoll(at(n)); }
  double d(size_t n) const { return std::stod(at(n)); }

 private:
  const std::string& at(size_t n) const {
    if (n >= captures.size()) throw StepFailure("step argument index " + std::to_string(n) + " out of range");
    return captures[n];
  }
};

/// Per-scenario state shared between steps. Fresh for every scenario.
class Context {
 public:
  template <class T, class... A>
  T& emplace(const std::string& key, A&&... a) {
    values_[key] = std::make_shared<std::any>(std::in_place_type<T>, std::forward<A>(a)...);
    return std::any_cast<T&>(*values_[key]);
  }
  template <class T>
  void set(const std::string& key, T value) { emplace<T>(key, std::move(value)); }
  /// Returns the value, default-constructing it on first access.
  template <class T>
  T& get(const std::string& key) {
    auto it = values_.find(key);
    if (it == values_.end()) return emplace<T>(key);
    T* p = std::any_cast<T>(it->second.get());
    if (!p) throw StepFailure("context key '" + key + "' holds a different type");
    return *p;
  }
  bool has(const std::string& key) const { return values_.count(key) != 0; }

 private:
  std::map<std::string, std::shared_ptr<std::any>> values_;
};

using StepFn = void (*)(Context& ctx, const StepArgs& args);

struct StepDef {
  std::string pattern;
  std::regex re;
  StepFn fn;
  const char* file;
  int line;
};

inline std::vector<StepDef>& registry() {
  static std::vector<StepDef> defs;
  return defs;
}

/// Mirror of expressionToRegex() in .gauntlet/lib/gherkin.mjs -- keep them identical.
inline std::string expression_to_regex(const std::string& expr) {
  if (!expr.empty() && expr.front() == '^' && expr.back() == '$') return expr;
  const std::string specials = ".*+?^${}()|[]\\";
  auto esc = [&](char c) { return specials.find(c) != std::string::npos ? std::string("\\") + c : std::string(1, c); };
  std::string out = "^";
  for (size_t i = 0; i < expr.size(); ++i) {
    char c = expr[i];
    if (c == '{') {
      size_t j = expr.find('}', i);
      if (j == std::string::npos) throw std::invalid_argument("unbalanced { in step pattern: " + expr);
      std::string name = expr.substr(i + 1, j - i - 1);
      if (name == "int") out += "(-?\\d+)";
      else if (name == "float") out += "(-?\\d+(?:\\.\\d+)?)";
      else if (name == "word") out += "(\\S+)";
      else if (name == "string") out += "\"([^\"]*)\"";
      else if (name.empty()) out += "(.*)";
      else throw std::invalid_argument("unknown parameter type {" + name + "} in: " + expr);
      i = j;
    } else if (c == '(') {
      size_t j = expr.find(')', i);
      if (j == std::string::npos) throw std::invalid_argument("unbalanced ( in step pattern: " + expr);
      out += "(?:";
      for (size_t k = i + 1; k < j; ++k) out += esc(expr[k]);
      out += ")?";
      i = j;
    } else if (c == '\\' && i + 1 < expr.size()) {
      out += esc(expr[++i]);
    } else {
      out += esc(c);
    }
  }
  return out + "$";
}

struct Registrar {
  Registrar(const char* pattern, StepFn fn, const char* file, int line) {
    registry().push_back({pattern, std::regex(expression_to_regex(pattern), std::regex::ECMAScript), fn, file, line});
  }
};

struct Step {
  std::string keyword, text, location, doc_string;
  bool has_doc_string;
  Table table;
};

struct Scenario {
  std::string id, feature, name, location;
  std::vector<Step> steps;
};

namespace detail {
template <class T>
auto show(const T& v, int) -> decltype(std::declval<std::ostream&>() << v, std::string()) {
  std::ostringstream os;
  os << v;
  return os.str();
}
template <class T>
std::string show(const T&, long) { return "<unprintable>"; }

inline std::string json_escape(const std::string& s) {
  std::string o;
  for (unsigned char c : s) {
    switch (c) {
      case '"': o += "\\\""; break;
      case '\\': o += "\\\\"; break;
      case '\n': o += "\\n"; break;
      case '\r': o += "\\r"; break;
      case '\t': o += "\\t"; break;
      default:
        if (c < 0x20) { char b[8]; std::snprintf(b, sizeof b, "\\u%04x", c); o += b; }
        else o += static_cast<char>(c);
    }
  }
  return o;
}
}  // namespace detail

#define GT_CONCAT_(a, b) a##b
#define GT_CONCAT(a, b) GT_CONCAT_(a, b)
#define GT_STEP_IMPL_(pattern, fn)                                                              \
  static void fn(::gauntlet::Context& ctx, const ::gauntlet::StepArgs& args);                   \
  static ::gauntlet::Registrar GT_CONCAT(fn, _reg)(pattern, &fn, __FILE__, __LINE__);           \
  static void fn([[maybe_unused]] ::gauntlet::Context& ctx, [[maybe_unused]] const ::gauntlet::StepArgs& args)
#define GT_STEP(pattern) GT_STEP_IMPL_(pattern, GT_CONCAT(gt_step_, __COUNTER__))

#define GT_FAIL(msg)                                                                              \
  throw ::gauntlet::StepFailure(std::string(__FILE__) + ":" + std::to_string(__LINE__) + ": " + (msg))
#define GT_EXPECT(cond) \
  do { if (!(cond)) GT_FAIL("expected " #cond); } while (0)
#define GT_EXPECT_EQ(actual, expected)                                                            \
  do {                                                                                            \
    const auto& gt_a_ = (actual);                                                                 \
    const auto& gt_e_ = (expected);                                                               \
    if (!(gt_a_ == gt_e_))                                                                        \
      GT_FAIL(std::string("expected " #actual " == " #expected "\n    actual:   ") +             \
              ::gauntlet::detail::show(gt_a_, 0) + "\n    expected: " + ::gauntlet::detail::show(gt_e_, 0)); \
  } while (0)
#define GT_EXPECT_THROWS(expr)                                                       \
  do {                                                                               \
    bool gt_threw_ = false;                                                          \
    try { (void)(expr); } catch (...) { gt_threw_ = true; }                          \
    if (!gt_threw_) GT_FAIL("expected exception from " #expr);                       \
  } while (0)

struct StepResult {
  std::string keyword, text, status, message;
};

inline int run_scenario(const Scenario& sc, std::vector<StepResult>& results) {
  Context ctx;
  bool failed = false;
  for (const auto& st : sc.steps) {
    StepResult r{st.keyword, st.text, "skipped", ""};
    if (failed) { results.push_back(r); continue; }
    std::vector<const StepDef*> hits;
    std::smatch m;
    const StepDef* hit = nullptr;
    StepArgs args;
    for (const auto& d : registry()) {
      std::smatch mm;
      if (std::regex_match(st.text, mm, d.re)) {
        hits.push_back(&d);
        if (!hit) { hit = &d; m = mm; }
      }
    }
    if (hits.empty()) {
      r.status = "undefined";
      r.message = "no step definition matches: " + st.text;
      failed = true;
    } else if (hits.size() > 1) {
      r.status = "ambiguous";
      r.message = "ambiguous step, " + std::to_string(hits.size()) + " definitions match";
      for (auto* h : hits) r.message += std::string("\n    ") + h->file + ":" + std::to_string(h->line) + " " + h->pattern;
      failed = true;
    } else {
      for (size_t k = 1; k < m.size(); ++k) args.captures.push_back(m[k].str());
      args.doc_string = st.doc_string;
      args.table = st.table;
      try {
        hit->fn(ctx, args);
        r.status = "passed";
      } catch (const std::exception& e) {
        r.status = "failed";
        r.message = e.what();
        failed = true;
      } catch (...) {
        r.status = "failed";
        r.message = "unknown exception";
        failed = true;
      }
    }
    results.push_back(r);
  }
  return failed ? 1 : 0;
}

inline void write_report(std::string dir, const Scenario& sc, const std::vector<StepResult>& results, int rc, double ms) {
  // GAUNTLET_REPORT_DIR overrides --report-dir; "-" disables reports (used by mutation runs).
  if (const char* env = std::getenv("GAUNTLET_REPORT_DIR")) dir = env;
  if (dir.empty() || dir == "-") return;
  std::string safe = sc.id;
  for (char& c : safe) if (c == '/' || c == '\\' || c == ':') c = '_';
  std::ofstream f(dir + "/" + safe + ".json", std::ios::binary);
  if (!f) { std::fprintf(stderr, "gauntlet: cannot write report into %s (does it exist?)\n", dir.c_str()); return; }
  using detail::json_escape;
  f << "{\"id\":\"" << json_escape(sc.id) << "\",\"feature\":\"" << json_escape(sc.feature) << "\",\"name\":\""
    << json_escape(sc.name) << "\",\"location\":\"" << json_escape(sc.location) << "\",\"status\":\""
    << (rc == 0 ? "passed" : "failed") << "\",\"ms\":" << ms << ",\"steps\":[";
  for (size_t i = 0; i < results.size(); ++i) {
    const auto& r = results[i];
    f << (i ? "," : "") << "{\"keyword\":\"" << json_escape(r.keyword) << "\",\"text\":\"" << json_escape(r.text)
      << "\",\"status\":\"" << r.status << "\",\"message\":\"" << json_escape(r.message) << "\"}";
  }
  f << "]}\n";
}

inline int run_main(int argc, char** argv, const std::vector<Scenario>& scenarios) {
  std::string only, report_dir;
  bool list = false;
  for (int i = 1; i < argc; ++i) {
    std::string a = argv[i];
    if (a == "--scenario" && i + 1 < argc) only = argv[++i];
    else if (a == "--report-dir" && i + 1 < argc) report_dir = argv[++i];
    else if (a == "--list") list = true;
  }
  if (list) {
    for (const auto& sc : scenarios) std::printf("%s\t%s\n", sc.id.c_str(), sc.name.c_str());
    return 0;
  }
  int failures = 0, ran = 0;
  for (const auto& sc : scenarios) {
    if (!only.empty() && sc.id != only) continue;
    ++ran;
    std::vector<StepResult> results;
    auto t0 = std::chrono::steady_clock::now();
    int rc = run_scenario(sc, results);
    double ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t0).count();
    write_report(report_dir, sc, results, rc, ms);
    std::printf("%s Scenario: %s  (%s)\n", rc == 0 ? "PASS" : "FAIL", sc.name.c_str(), sc.location.c_str());
    for (const auto& r : results) {
      const char* mark = r.status == "passed" ? "  ok " : r.status == "skipped" ? "  -- " : "  !! ";
      std::printf("%s%s %s\n", mark, r.keyword.c_str(), r.text.c_str());
      if (!r.message.empty()) std::printf("       %s\n", r.message.c_str());
    }
    failures += rc;
  }
  if (ran == 0) {
    std::fprintf(stderr, "no scenario matched '%s'\n", only.c_str());
    return 2;
  }
  return failures ? 1 : 0;
}

}  // namespace gauntlet
