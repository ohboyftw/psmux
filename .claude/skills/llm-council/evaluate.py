"""
Benchmarking harness — score each review source against ground truth.

Scores EVERY scorable layer independently:
  - Each individual LLM (claude, gpt-4o, gemini, minimax, kimi-k25)
  - CodeRabbit standalone
  - Council synthesis (LLM consensus)
  - Grand synthesis (council + CodeRabbit merged)

Metrics: Precision, Recall, F1, Severity/Category accuracy, Latency, Bootstrap 95% CI
"""

import asyncio, json, math, random, time
from dataclasses import dataclass, field, asdict
from pathlib import Path
from typing import Optional

from council import (
    CouncilResult, Finding, MemberConfig, Provider, SourceReview,
    SynthesisResult, extract_findings_by_source, load_cached_coderabbit,
    run_council,
)

# ---------------------------------------------------------------------------
# Ground truth
# ---------------------------------------------------------------------------

@dataclass
class ExpectedFinding:
    category: str; severity: str; file: str; line: Optional[int]; title: str; description: str

@dataclass
class TestCase:
    id: str; name: str; description: str; diff: str
    expected_findings: list[ExpectedFinding]
    tags: list[str] = field(default_factory=list)

BUILTIN_CASES = [
    TestCase(
        id="sql-injection", name="SQL Injection via String Interpolation",
        description="Parameterized query replaced with f-string",
        diff="""diff --git a/app/db.py b/app/db.py
--- a/app/db.py
+++ b/app/db.py
@@ -10,7 +10,12 @@ import sqlite3
 
 def get_user(username):
     conn = sqlite3.connect('users.db')
-    return conn.execute("SELECT * FROM users WHERE name = ?", (username,))
+    query = f"SELECT * FROM users WHERE name = '{username}'"
+    return conn.execute(query)
+
+def delete_user(user_id):
+    conn = sqlite3.connect('users.db')
+    conn.execute(f"DELETE FROM users WHERE id = {user_id}")
""",
        expected_findings=[
            ExpectedFinding("security", "critical", "app/db.py", 13, "SQL Injection", "f-string in SELECT"),
            ExpectedFinding("security", "critical", "app/db.py", 17, "SQL Injection in DELETE", "f-string in DELETE"),
        ], tags=["security"],
    ),
    TestCase(
        id="race-condition", name="Race Condition in Counter",
        description="Non-atomic read-modify-write without lock",
        diff="""diff --git a/counter/service.py b/counter/service.py
--- a/counter/service.py
+++ b/counter/service.py
@@ -1,10 +1,20 @@
+import threading
+
 class CounterService:
     def __init__(self):
         self.count = 0
+        self._lock = threading.Lock()
 
     def increment(self):
-        with self._lock:
-            self.count += 1
+        current = self.count
+        self.count = current + 1
+        return self.count
+
+    def get_and_reset(self):
+        val = self.count
+        self.count = 0
+        return val
""",
        expected_findings=[
            ExpectedFinding("correctness", "critical", "counter/service.py", 11, "Race condition in increment", "Non-atomic RMW"),
            ExpectedFinding("correctness", "major", "counter/service.py", 16, "Race condition in get_and_reset", "Non-atomic check-then-act"),
        ], tags=["concurrency"],
    ),
    TestCase(
        id="react-memory-leak", name="React useEffect Memory Leak",
        description="Missing cleanup for listeners and intervals",
        diff="""diff --git a/src/Dashboard.jsx b/src/Dashboard.jsx
--- a/src/Dashboard.jsx
+++ b/src/Dashboard.jsx
@@ -5,10 +5,15 @@ function Dashboard({ userId }) {
   const [data, setData] = useState(null);
 
   useEffect(() => {
-    const controller = new AbortController();
-    fetch(`/api/data/${userId}`, { signal: controller.signal })
+    fetch(`/api/data/${userId}`)
       .then(r => r.json())
       .then(setData);
-    return () => controller.abort();
+    
+    window.addEventListener('resize', handleResize);
+    const interval = setInterval(fetchUpdates, 5000);
   }, [userId]);
+
+  const handleResize = () => setData(prev => ({...prev, width: window.innerWidth}));
+  const fetchUpdates = () => fetch(`/api/updates/${userId}`).then(r => r.json()).then(setData);
""",
        expected_findings=[
            ExpectedFinding("correctness", "major", "src/Dashboard.jsx", 8, "Missing AbortController", "Fetch no longer cancels on unmount"),
            ExpectedFinding("correctness", "major", "src/Dashboard.jsx", 11, "Memory leak — no cleanup", "resize listener and interval never removed"),
            ExpectedFinding("correctness", "minor", "src/Dashboard.jsx", 15, "Stale closure risk", "Functions after useEffect — stale closure"),
        ], tags=["react", "memory"],
    ),
    TestCase(
        id="auth-bypass", name="Missing Auth on Admin Endpoint",
        description="New admin endpoint lacks @require_admin decorator",
        diff="""diff --git a/routes/admin.py b/routes/admin.py
--- a/routes/admin.py
+++ b/routes/admin.py
@@ -12,9 +12,14 @@ from auth import require_admin, require_login
 @app.route('/admin/users')
 @require_admin
 def list_users():
     return jsonify(User.query.all())
 
+@app.route('/admin/export', methods=['POST'])
+def export_all_data():
+    data = User.query.all()
+    return generate_csv(data)
+
 @app.route('/admin/settings', methods=['PUT'])
 @require_admin
 def update_settings():
""",
        expected_findings=[
            ExpectedFinding("security", "critical", "routes/admin.py", 17, "Missing authentication", "No auth decorator on export_all_data"),
        ], tags=["security", "auth"],
    ),
    TestCase(
        id="n-plus-one", name="N+1 Query Regression",
        description="Eager loading replaced with per-row queries",
        diff="""diff --git a/api/views.py b/api/views.py
--- a/api/views.py
+++ b/api/views.py
@@ -15,10 +15,15 @@ from models import Order, Customer, Product
 
 def get_order_report():
-    orders = Order.query.options(joinedload(Order.customer), joinedload(Order.items)).all()
+    orders = Order.query.all()
     result = []
     for order in orders:
-        result.append({
+        customer = Customer.query.get(order.customer_id)
+        items = Product.query.filter(Product.order_id == order.id).all()
+        result.append({
             "id": order.id,
-            "customer": order.customer.name,
-            "items": [i.name for i in order.items],
+            "customer": customer.name,
+            "items": [i.name for i in items],
+            "total": sum(i.price for i in items),
         })
     return result
""",
        expected_findings=[
            ExpectedFinding("performance", "major", "api/views.py", 20, "N+1 query — Customer", "Per-row Customer.query.get"),
            ExpectedFinding("performance", "major", "api/views.py", 21, "N+1 query — Product", "Per-row Product.query.filter"),
        ], tags=["performance", "database"],
    ),
]

# ---------------------------------------------------------------------------
# Scoring
# ---------------------------------------------------------------------------

@dataclass
class FindingMatchDetail:
    expected_title: str; found: bool; matched_to: Optional[str] = None
    severity_match: bool = False; category_match: bool = False

@dataclass
class SourceScore:
    test_case_id: str; source_id: str; tp: int; fp: int; fn: int
    precision: float; recall: float; f1: float
    severity_accuracy: float; category_accuracy: float
    n_findings_reported: int; latency_ms: int
    details: list[FindingMatchDetail] = field(default_factory=list)

def _norm(s: str) -> str:
    return s.lower().strip().replace("-", " ").replace("_", " ")

def _match(exp: ExpectedFinding, act: dict) -> bool:
    ef = exp.file.split("/")[-1].lower()
    af = act.get("file", "").split("/")[-1].lower()
    if ef != af: return False
    cat = _norm(exp.category) == _norm(act.get("category", ""))
    ew = set(_norm(exp.title).split())
    aw = set(_norm(act.get("title", "")).split()) | set(_norm(act.get("description", "")).split())
    return cat or len(ew & aw) >= 1

def score_source(tc_id, src_id, expected, actual, latency_ms=0) -> SourceScore:
    used = set(); details = []
    for exp in expected:
        found = False; mt = None; sev_ok = cat_ok = False
        for i, act in enumerate(actual):
            if i in used: continue
            if _match(exp, act):
                found = True; mt = act.get("title", "?")
                sev_ok = _norm(exp.severity) == _norm(act.get("severity", ""))
                cat_ok = _norm(exp.category) == _norm(act.get("category", ""))
                used.add(i); break
        details.append(FindingMatchDetail(exp.title, found, mt, sev_ok, cat_ok))

    tp = sum(1 for d in details if d.found)
    fn = sum(1 for d in details if not d.found)
    fp = len(actual) - len(used)
    p = tp / (tp + fp) if (tp + fp) else 0.0
    r = tp / (tp + fn) if (tp + fn) else 0.0
    f1 = 2*p*r / (p+r) if (p+r) else 0.0
    sa = sum(1 for d in details if d.found and d.severity_match) / tp if tp else 0.0
    ca = sum(1 for d in details if d.found and d.category_match) / tp if tp else 0.0
    return SourceScore(tc_id, src_id, tp, fp, fn, p, r, f1, sa, ca, len(actual), latency_ms, details)

# ---------------------------------------------------------------------------
# Aggregation
# ---------------------------------------------------------------------------

@dataclass
class AggregateScore:
    source_id: str; n_cases: int
    macro_precision: float; macro_recall: float; macro_f1: float
    macro_severity_acc: float; macro_category_acc: float
    total_tp: int; total_fp: int; total_fn: int
    micro_precision: float; micro_recall: float; micro_f1: float
    avg_latency_ms: int; f1_ci_low: float = 0.0; f1_ci_high: float = 0.0

def _bootstrap_ci(vals, n_boot=1000, ci=0.95):
    if len(vals) < 2: return (vals[0] if vals else 0, vals[0] if vals else 0)
    means = sorted([sum(random.choices(vals, k=len(vals)))/len(vals) for _ in range(n_boot)])
    lo, hi = int((1-ci)/2*n_boot), int((1+ci)/2*n_boot)-1
    return means[lo], means[hi]

def aggregate(scores, source_id) -> AggregateScore:
    n = len(scores)
    if not n: return AggregateScore(source_id, 0, 0,0,0,0,0,0,0,0,0,0,0,0)
    mp = sum(s.precision for s in scores)/n
    mr = sum(s.recall for s in scores)/n
    mf = sum(s.f1 for s in scores)/n
    ms = sum(s.severity_accuracy for s in scores)/n
    mc = sum(s.category_accuracy for s in scores)/n
    ttp, tfp, tfn = sum(s.tp for s in scores), sum(s.fp for s in scores), sum(s.fn for s in scores)
    mip = ttp/(ttp+tfp) if (ttp+tfp) else 0
    mir = ttp/(ttp+tfn) if (ttp+tfn) else 0
    mif = 2*mip*mir/(mip+mir) if (mip+mir) else 0
    al = sum(s.latency_ms for s in scores)//n
    ci_lo, ci_hi = _bootstrap_ci([s.f1 for s in scores])
    return AggregateScore(source_id, n, mp, mr, mf, ms, mc, ttp, tfp, tfn, mip, mir, mif, al, ci_lo, ci_hi)

# ---------------------------------------------------------------------------
# Evaluation pipeline
# ---------------------------------------------------------------------------

@dataclass
class EvalResult:
    per_source_per_case: dict[str, list[SourceScore]]
    aggregates: dict[str, AggregateScore]
    test_case_ids: list[str]

async def run_evaluation(test_cases=None, council_members=None,
                         include_coderabbit=True, cr_cache_dir="eval_data/coderabbit_captures"):
    cases = test_cases or BUILTIN_CASES
    all_scores: dict[str, list[SourceScore]] = {}

    for i, tc in enumerate(cases, 1):
        print(f"\n[{i}/{len(cases)}] {tc.name} ({tc.id})")
        cached_cr = load_cached_coderabbit(tc.id, cr_cache_dir)
        if cached_cr:
            print(f"  📦 Loaded cached CodeRabbit ({len(cached_cr.findings)} findings)")

        result = await run_council(
            tc.diff, council=council_members,
            include_coderabbit=include_coderabbit and cached_cr is None,
            cached_coderabbit=cached_cr)

        sources = extract_findings_by_source(result)
        latency_map = {r.source_id: r.latency_ms for r in result.individual_reviews}
        if result.council_synthesis: latency_map["council-synthesis"] = result.council_synthesis.latency_ms
        if result.grand_synthesis: latency_map["grand-synthesis"] = result.grand_synthesis.latency_ms

        for src_id, findings in sources.items():
            score = score_source(tc.id, src_id, tc.expected_findings, findings, latency_map.get(src_id, 0))
            all_scores.setdefault(src_id, []).append(score)
            e = "✅" if score.recall >= 0.5 else "⚠️"
            print(f"  {e} {src_id:25s} P={score.precision:.2f} R={score.recall:.2f} "
                  f"F1={score.f1:.2f} ({score.tp}TP {score.fp}FP {score.fn}FN)")

    aggs = {sid: aggregate(scores, sid) for sid, scores in all_scores.items()}
    return EvalResult(all_scores, aggs, [tc.id for tc in cases])

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------

def format_eval_report(result: EvalResult) -> str:
    lines = ["# 📊 LLM Council Benchmark Report\n"]
    lines.append(f"**Test cases:** {len(result.test_case_ids)}")
    lines.append(f"**Sources evaluated:** {', '.join(sorted(result.aggregates.keys()))}\n")

    # Leaderboard
    lines.append("## 🏆 Aggregate Leaderboard\n")
    lines.append("| Rank | Source | Macro F1 | 95% CI | Precision | Recall | Sev Acc | Cat Acc | Latency |")
    lines.append("|------|--------|----------|--------|-----------|--------|---------|---------|---------|")
    ranked = sorted(result.aggregates.values(), key=lambda a: a.macro_f1, reverse=True)
    for rank, a in enumerate(ranked, 1):
        medal = {1:"🥇",2:"🥈",3:"🥉"}.get(rank, str(rank))
        lines.append(f"| {medal} | **{a.source_id}** | {a.macro_f1:.2%} | [{a.f1_ci_low:.2%}, {a.f1_ci_high:.2%}] | "
                      f"{a.macro_precision:.2%} | {a.macro_recall:.2%} | {a.macro_severity_acc:.2%} | "
                      f"{a.macro_category_acc:.2%} | {a.avg_latency_ms}ms |")

    # Micro stats
    lines.append("\n## 📈 Micro-Averaged Stats\n")
    lines.append("| Source | TP | FP | FN | Micro P | Micro R | Micro F1 |")
    lines.append("|--------|----|----|----|---------|---------| ---------|")
    for a in ranked:
        lines.append(f"| {a.source_id} | {a.total_tp} | {a.total_fp} | {a.total_fn} | "
                      f"{a.micro_precision:.2%} | {a.micro_recall:.2%} | {a.micro_f1:.2%} |")

    # Head-to-head
    sids = [a.source_id for a in ranked]
    if len(sids) >= 2:
        lines.append("\n## ⚔️ Head-to-Head F1 Deltas\n")
        lines.append("| |" + "|".join(f" {s} " for s in sids) + "|")
        lines.append("|" + "|".join(["---"]*(len(sids)+1)) + "|")
        for a in ranked:
            row = f"| **{a.source_id}** |"
            for b_id in sids:
                b = result.aggregates[b_id]
                d = a.macro_f1 - b.macro_f1
                row += " — |" if a.source_id == b_id else f" {'🟢' if d>0.01 else '🔴' if d<-0.01 else '⚪'} {d:+.2%} |"
            lines.append(row)

    # Heatmap
    lines.append("\n## 🗺️ Per-Case Recall Heatmap\n")
    lines.append("| Test Case |" + "|".join(f" {s} " for s in sids) + "|")
    lines.append("|" + "|".join(["---"]*(len(sids)+1)) + "|")
    for tc_id in result.test_case_ids:
        row = f"| {tc_id} |"
        for sid in sids:
            sc = next((s for s in result.per_source_per_case.get(sid, []) if s.test_case_id == tc_id), None)
            if sc:
                e = "🟢" if sc.recall>=0.8 else ("🟡" if sc.recall>=0.5 else "🔴")
                row += f" {e} {sc.recall:.0%} |"
            else:
                row += " ⬜ N/A |"
        lines.append(row)

    # Insights
    lines.append("\n## 💡 Key Insights\n")
    if ranked:
        lines.append(f"- **Best overall:** {ranked[0].source_id} (F1={ranked[0].macro_f1:.2%})")
        br = max(ranked, key=lambda a: a.macro_recall)
        bp = max(ranked, key=lambda a: a.macro_precision)
        lines.append(f"- **Highest recall:** {br.source_id} ({br.macro_recall:.2%})")
        lines.append(f"- **Highest precision:** {bp.source_id} ({bp.macro_precision:.2%})")
        for sid in ["council-synthesis", "grand-synthesis"]:
            if sid in result.aggregates:
                sa = result.aggregates[sid]
                indiv = [a.macro_f1 for a in ranked if a.source_id not in ["council-synthesis","grand-synthesis"]]
                if indiv:
                    delta = sa.macro_f1 - max(indiv)
                    lines.append(f"- **{sid} vs best individual:** {delta:+.2%} — {'adds value ✅' if delta>0 else 'underperforms ⚠️'}")

    return "\n".join(lines)

def export_json(result: EvalResult) -> str:
    return json.dumps({
        "aggregates": {sid: {"macro_f1": a.macro_f1, "macro_precision": a.macro_precision,
            "macro_recall": a.macro_recall, "micro_f1": a.micro_f1,
            "tp": a.total_tp, "fp": a.total_fp, "fn": a.total_fn,
            "avg_latency_ms": a.avg_latency_ms, "f1_ci": [a.f1_ci_low, a.f1_ci_high]}
            for sid, a in result.aggregates.items()},
        "leaderboard": [{"rank": i+1, "source": a.source_id, "f1": a.macro_f1}
            for i, a in enumerate(sorted(result.aggregates.values(), key=lambda x: x.macro_f1, reverse=True))],
    }, indent=2)

# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

async def main():
    import argparse
    p = argparse.ArgumentParser(description="Benchmark LLM Council")
    p.add_argument("--output", "-o"); p.add_argument("--json", action="store_true")
    p.add_argument("--cases", nargs="*"); p.add_argument("--no-coderabbit", action="store_true")
    p.add_argument("--cr-cache", default="eval_data/coderabbit_captures")
    p.add_argument("--models", nargs="*")
    args = p.parse_args()

    cases = BUILTIN_CASES
    if args.cases: cases = [tc for tc in cases if tc.id in args.cases]
    council = None
    if args.models:
        council = [MemberConfig(f"{s.split(':')[0]}:{s.split(':')[1]}", Provider(s.split(':')[0]), s.split(':')[1])
                   for s in args.models]

    print("🏛️ LLM Council Benchmark")
    print(f"   Cases: {len(cases)} | CodeRabbit: {'on' if not args.no_coderabbit else 'off'}")
    result = await run_evaluation(cases, council, not args.no_coderabbit, args.cr_cache)
    output = export_json(result) if args.json else format_eval_report(result)

    if args.output:
        Path(args.output).parent.mkdir(parents=True, exist_ok=True)
        Path(args.output).write_text(output)
        print(f"\n📄 Report: {args.output}")
    else:
        print("\n" + output)

if __name__ == "__main__":
    asyncio.run(main())
