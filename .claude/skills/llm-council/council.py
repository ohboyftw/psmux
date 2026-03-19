"""
LLM Council + CodeRabbit — Unified multi-source code review engine.

5 LLM Council Members:
  - Claude (Anthropic) — claude-sonnet-4-20250514
  - GPT-5.2 (OpenAI)
  - Gemini Flash (Google)
  - MiniMax-M2.1 (MiniMax) — OpenAI-compatible API at api.minimax.io
  - Kimi K2.5 (Moonshot AI) — OpenAI-compatible API at api.moonshot.cn

+ CodeRabbit as an independent review source

Pipeline:
  1. Fan-out to all LLMs + CodeRabbit in parallel
  2. Council synthesis (LLM consensus only)
  3. Grand synthesis (council consensus + CodeRabbit)
  4. Every layer independently scorable against ground truth
"""

import asyncio
import json
import os
import subprocess
import time
from dataclasses import dataclass, field, asdict
from enum import Enum
from pathlib import Path
from typing import Optional


# ---------------------------------------------------------------------------
# Data structures
# ---------------------------------------------------------------------------

class Provider(Enum):
    ANTHROPIC = "anthropic"
    OPENAI = "openai"
    GOOGLE = "google"
    MINIMAX = "minimax"
    MOONSHOT = "moonshot"     # Kimi K2.5
    CODERABBIT = "coderabbit"
    OPENROUTER = "openrouter" # Fallback for any model via OpenRouter


@dataclass
class Finding:
    severity: str       # critical | major | minor | suggestion
    category: str       # security | performance | correctness | style | architecture | testing
    file: str
    line: Optional[int]
    title: str
    description: str
    suggestion: Optional[str] = None

    def to_dict(self) -> dict:
        return {k: v for k, v in asdict(self).items() if v is not None}


@dataclass
class SourceReview:
    source_id: str
    source_type: str        # "llm" | "coderabbit"
    model: str
    findings: list[Finding]
    summary: str
    confidence: float
    latency_ms: int
    raw_response: str = ""
    error: Optional[str] = None


@dataclass
class SynthesisResult:
    synthesis_id: str
    sources_used: list[str]
    consensus_findings: list[dict]
    disputed_findings: list[dict]
    summary: str
    recommendation: str     # approve | request-changes | needs-discussion
    agreement_score: float
    latency_ms: int


@dataclass
class CouncilResult:
    individual_reviews: list[SourceReview]
    council_synthesis: Optional[SynthesisResult]
    grand_synthesis: Optional[SynthesisResult]
    total_latency_ms: int


# ---------------------------------------------------------------------------
# Prompts
# ---------------------------------------------------------------------------

REVIEW_SYSTEM = """You are a senior code reviewer. Analyze the diff and return ONLY valid JSON:
{
  "findings": [
    {
      "severity": "critical|major|minor|suggestion",
      "category": "security|performance|correctness|style|architecture|testing|documentation",
      "file": "path/to/file",
      "line": null_or_number,
      "title": "Short title",
      "description": "Why this is a problem",
      "suggestion": "How to fix (optional)"
    }
  ],
  "summary": "2-3 sentence assessment",
  "confidence": 0.0_to_1.0
}
Focus on correctness, security, and architecture. Be specific with line references.
Return ONLY JSON — no markdown fences, no commentary."""

COUNCIL_SYNTHESIS_SYSTEM = """You synthesize reviews from {n} LLM reviewers into a consensus report.
Identify:
1. CONSENSUS findings (2+ reviewers agree on the same issue)
2. DISPUTED findings (only 1 reviewer flagged)
For disputed items, assess validity using your expertise.

Return ONLY JSON:
{{
  "consensus_findings": [
    {{"severity":"...","category":"...","file":"...","line":null,"title":"...","description":"...","suggestion":"...","agreed_by":["model1","model2"]}}
  ],
  "disputed_findings": [
    {{"severity":"...","category":"...","file":"...","line":null,"title":"...","description":"...","suggestion":"...","raised_by":"model","verdict":"valid|invalid|uncertain","reasoning":"..."}}
  ],
  "summary": "...",
  "recommendation": "approve|request-changes|needs-discussion",
  "agreement_score": 0.0_to_1.0
}}"""

GRAND_SYNTHESIS_SYSTEM = """You are the lead engineer producing the FINAL review by merging:
1. LLM Council consensus (multi-model agreement from {n} sources)
2. CodeRabbit findings (production code review tool with AST analysis)

Where both agree → HIGH CONFIDENCE. Where only one found something → assess validity.
CodeRabbit has AST-level analysis; Council has multi-perspective reasoning.

Return ONLY JSON:
{{
  "consensus_findings": [
    {{"severity":"...","category":"...","file":"...","line":null,"title":"...","description":"...","suggestion":"...","sources":["council","coderabbit"]}}
  ],
  "disputed_findings": [
    {{"severity":"...","category":"...","file":"...","line":null,"title":"...","description":"...","suggestion":"...","source":"council_or_coderabbit","verdict":"valid|invalid|uncertain","reasoning":"..."}}
  ],
  "summary": "...",
  "recommendation": "approve|request-changes|needs-discussion",
  "agreement_score": 0.0_to_1.0
}}"""


# ---------------------------------------------------------------------------
# Provider clients
# ---------------------------------------------------------------------------

def _parse_json(raw: str) -> dict:
    text = raw.strip()
    if text.startswith("```"):
        lines = text.split("\n")
        start, end = 1, len(lines) - 1
        for i in range(len(lines) - 1, 0, -1):
            if lines[i].strip() == "```":
                end = i
                break
        text = "\n".join(lines[start:end])
    return json.loads(text)


async def _call_anthropic(model: str, system: str, user: str) -> str:
    """Anthropic native API."""
    try:
        import anthropic
        client = anthropic.AsyncAnthropic()
        r = await client.messages.create(
            model=model, max_tokens=4096, system=system,
            messages=[{"role": "user", "content": user}])
        return r.content[0].text
    except ImportError:
        import httpx
        async with httpx.AsyncClient(timeout=120) as c:
            r = await c.post("https://api.anthropic.com/v1/messages", headers={
                "x-api-key": os.environ["ANTHROPIC_API_KEY"],
                "anthropic-version": "2023-06-01",
                "content-type": "application/json",
            }, json={"model": model, "max_tokens": 4096, "system": system,
                     "messages": [{"role": "user", "content": user}]})
            r.raise_for_status()
            return r.json()["content"][0]["text"]


async def _call_openai(model: str, system: str, user: str) -> str:
    """OpenAI native API. Uses Responses API for codex models, Chat Completions otherwise."""
    import httpx
    api_key = os.environ["OPENAI_API_KEY"]
    headers = {"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"}

    if "codex" in model.lower():
        # Codex models require the Responses API (/v1/responses)
        async with httpx.AsyncClient(timeout=180) as c:
            r = await c.post("https://api.openai.com/v1/responses", headers=headers,
                json={"model": model, "max_output_tokens": 4096,
                      "input": [{"role": "developer", "content": system},
                                {"role": "user", "content": user}]})
            r.raise_for_status()
            data = r.json()
            for item in data.get("output", []):
                if item.get("type") == "message":
                    for block in item.get("content", []):
                        if block.get("type") == "output_text":
                            return block["text"]
            raise ValueError(f"No text output in Responses API result: {data.get('output', [])}")

    # Standard Chat Completions API
    try:
        from openai import AsyncOpenAI
        client = AsyncOpenAI()
        r = await client.chat.completions.create(
            model=model, max_tokens=4096, temperature=0.2,
            messages=[{"role": "system", "content": system},
                      {"role": "user", "content": user}])
        return r.choices[0].message.content
    except ImportError:
        async with httpx.AsyncClient(timeout=120) as c:
            r = await c.post("https://api.openai.com/v1/chat/completions", headers=headers,
                json={"model": model, "max_tokens": 4096, "temperature": 0.2,
                      "messages": [{"role": "system", "content": system},
                                   {"role": "user", "content": user}]})
            r.raise_for_status()
            return r.json()["choices"][0]["message"]["content"]


async def _call_google(model: str, system: str, user: str) -> str:
    """Google Gemini API."""
    try:
        from google import genai
        client = genai.Client()
        r = await asyncio.to_thread(
            client.models.generate_content, model=model,
            contents=f"{system}\n\n{user}")
        return r.text
    except ImportError:
        import httpx
        key = os.environ["GOOGLE_API_KEY"]
        async with httpx.AsyncClient(timeout=120) as c:
            r = await c.post(
                f"https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={key}",
                json={"contents": [{"parts": [{"text": f"{system}\n\n{user}"}]}],
                      "generationConfig": {"maxOutputTokens": 4096, "temperature": 0.2}})
            r.raise_for_status()
            return r.json()["candidates"][0]["content"]["parts"][0]["text"]


async def _call_openai_compatible(
    base_url: str, api_key: str, model: str, system: str, user: str,
    temperature: float = 0.2, max_tokens: int = 4096,
) -> str:
    """Generic OpenAI-compatible API caller. Used by MiniMax, Moonshot, OpenRouter."""
    import re
    import httpx
    async with httpx.AsyncClient(timeout=180) as c:
        r = await c.post(
            f"{base_url}/chat/completions",
            headers={
                "Authorization": f"Bearer {api_key}",
                "Content-Type": "application/json",
            },
            json={
                "model": model,
                "max_tokens": max_tokens,
                "temperature": temperature,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user},
                ],
            },
        )
        r.raise_for_status()
        content = r.json()["choices"][0]["message"]["content"]
        # Strip <think>...</think> tags from reasoning models (e.g. MiniMax M2.1)
        content = re.sub(r"<think>.*?</think>\s*", "", content, flags=re.DOTALL)
        return content


async def _call_minimax(model: str, system: str, user: str) -> str:
    """
    MiniMax API — OpenAI-compatible at api.minimax.io/v1
    Models: MiniMax-M1-40k, MiniMax-M1-80k, MiniMax-M2.1
    Env: MINIMAX_API_KEY
    """
    api_key = os.environ.get("MINIMAX_API_KEY")
    if not api_key:
        raise ValueError("MINIMAX_API_KEY not set")
    base_url = os.environ.get("MINIMAX_API_BASE", "https://api.minimax.io/v1")
    return await _call_openai_compatible(base_url, api_key, model, system, user)


async def _call_moonshot(model: str, system: str, user: str) -> str:
    """
    Kimi K2.5 (Moonshot AI) — OpenAI-compatible at api.moonshot.ai/v1
    Models: kimi-k2.5, kimi-k2-instruct
    Env: MOONSHOT_API_KEY
    Kimi K2.5 requires temperature=1.0 (thinking mode).
    """
    api_key = os.environ.get("MOONSHOT_API_KEY")
    if not api_key:
        raise ValueError("MOONSHOT_API_KEY not set")
    base_url = os.environ.get("MOONSHOT_API_BASE", "https://api.moonshot.ai/v1")
    return await _call_openai_compatible(
        base_url, api_key, model, system, user, temperature=1.0)


async def _call_openrouter(model: str, system: str, user: str) -> str:
    """
    OpenRouter — universal fallback for any model.
    Env: OPENROUTER_API_KEY
    Model format: "provider/model-name" e.g. "minimax/minimax-m1", "moonshotai/kimi-k2.5"
    """
    api_key = os.environ.get("OPENROUTER_API_KEY")
    if not api_key:
        raise ValueError("OPENROUTER_API_KEY not set")
    return await _call_openai_compatible(
        "https://openrouter.ai/api/v1", api_key, model, system, user)


_LLM_DISPATCH = {
    Provider.ANTHROPIC:  _call_anthropic,
    Provider.OPENAI:     _call_openai,
    Provider.GOOGLE:     _call_google,
    Provider.MINIMAX:    _call_minimax,
    Provider.MOONSHOT:   _call_moonshot,
    Provider.OPENROUTER: _call_openrouter,
}


# ---------------------------------------------------------------------------
# CodeRabbit client
# ---------------------------------------------------------------------------

async def _call_coderabbit(diff: str, config: dict) -> SourceReview:
    t0 = time.monotonic()

    # API mode
    api_key = os.environ.get("CODERABBIT_API_KEY")
    if api_key:
        try:
            import httpx
            async with httpx.AsyncClient(timeout=180) as c:
                r = await c.post(
                    config.get("api_url", "https://api.coderabbit.ai/v1/reviews"),
                    headers={"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"},
                    json={"diff": diff, "format": "json"})
                r.raise_for_status()
                data = r.json()
                findings = [Finding(**f) for f in data.get("findings", data.get("comments", []))]
                return SourceReview(
                    source_id="coderabbit", source_type="coderabbit", model="coderabbit-api",
                    findings=findings, summary=data.get("summary", ""),
                    confidence=0.85, latency_ms=int((time.monotonic() - t0) * 1000),
                    raw_response=json.dumps(data))
        except Exception as e:
            return SourceReview(
                source_id="coderabbit", source_type="coderabbit", model="coderabbit-api",
                findings=[], summary="", confidence=0.0,
                latency_ms=int((time.monotonic() - t0) * 1000), error=f"API error: {e}")

    # CLI mode
    try:
        which = await asyncio.create_subprocess_exec(
            "which", "cr", stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
        await which.communicate()
        if which.returncode == 0:
            tmp = Path("/tmp/_council_cr.patch")
            tmp.write_text(diff)
            proc = await asyncio.create_subprocess_exec(
                "cr", "review", "--diff", str(tmp), "--format", "json",
                stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
            stdout, _ = await asyncio.wait_for(proc.communicate(), timeout=180)
            data = json.loads(stdout.decode())
            findings = []
            for f in data.get("findings", data.get("comments", [])):
                try:
                    findings.append(Finding(**{k: f.get(k) for k in
                        ["severity", "category", "file", "line", "title", "description", "suggestion"]}))
                except TypeError:
                    pass
            return SourceReview(
                source_id="coderabbit", source_type="coderabbit", model="coderabbit-cli",
                findings=findings, summary=data.get("summary", ""),
                confidence=0.85, latency_ms=int((time.monotonic() - t0) * 1000),
                raw_response=json.dumps(data))
    except Exception:
        pass

    return SourceReview(
        source_id="coderabbit", source_type="coderabbit", model="coderabbit-unavailable",
        findings=[], summary="", confidence=0.0, latency_ms=0,
        error="CodeRabbit not available. Set CODERABBIT_API_KEY or install `cr` CLI.")


def load_cached_coderabbit(test_case_id: str, cache_dir: str = "eval_data/coderabbit_captures") -> Optional[SourceReview]:
    path = Path(cache_dir) / f"{test_case_id}.json"
    if not path.exists():
        return None
    data = json.loads(path.read_text())
    findings = [Finding(**f) for f in data.get("findings", [])]
    return SourceReview(
        source_id="coderabbit", source_type="coderabbit",
        model=data.get("model", "coderabbit-cached"),
        findings=findings, summary=data.get("summary", ""),
        confidence=data.get("confidence", 0.85),
        latency_ms=data.get("latency_ms", 0),
        raw_response=json.dumps(data))


# ---------------------------------------------------------------------------
# Council member configuration
# ---------------------------------------------------------------------------

@dataclass
class MemberConfig:
    source_id: str
    provider: Provider
    model: str
    weight: float = 1.0


DEFAULT_COUNCIL = [
    MemberConfig("claude",   Provider.ANTHROPIC, "claude-sonnet-4-20250514", 1.0),
    MemberConfig("gpt-53",   Provider.OPENAI,    "gpt-5.2-codex",            0.9),
    MemberConfig("gemini",   Provider.GOOGLE,     "gemini-2.0-flash",         0.85),
    MemberConfig("minimax",  Provider.MINIMAX,    "MiniMax-M2.1",             0.8),
    MemberConfig("kimi-k25", Provider.MOONSHOT,   "kimi-k2.5",               0.8),
]

DEFAULT_SYNTHESIZER = MemberConfig("synthesizer", Provider.ANTHROPIC, "claude-sonnet-4-20250514")


# ---------------------------------------------------------------------------
# Review execution
# ---------------------------------------------------------------------------

async def _review_llm(member: MemberConfig, diff: str, context: str) -> SourceReview:
    user_msg = f"## Code Diff\n```diff\n{diff}\n```"
    if context:
        user_msg = f"## Context\n{context}\n\n{user_msg}"

    t0 = time.monotonic()
    call_fn = _LLM_DISPATCH[member.provider]

    try:
        raw = await call_fn(member.model, REVIEW_SYSTEM, user_msg)
    except Exception as e:
        return SourceReview(
            source_id=member.source_id, source_type="llm", model=member.model,
            findings=[], summary="", confidence=0.0,
            latency_ms=int((time.monotonic() - t0) * 1000), error=str(e))

    latency = int((time.monotonic() - t0) * 1000)
    try:
        data = _parse_json(raw)
        findings = [Finding(**f) for f in data.get("findings", [])]
        return SourceReview(
            source_id=member.source_id, source_type="llm", model=member.model,
            findings=findings, summary=data.get("summary", ""),
            confidence=float(data.get("confidence", 0.5)),
            latency_ms=latency, raw_response=raw)
    except (json.JSONDecodeError, TypeError, KeyError) as e:
        return SourceReview(
            source_id=member.source_id, source_type="llm", model=member.model,
            findings=[], summary="", confidence=0.0,
            latency_ms=latency, raw_response=raw, error=f"Parse error: {e}")


async def _synthesize(
    reviews: list[SourceReview], diff: str, synth: MemberConfig,
    system_template: str, synthesis_id: str,
) -> SynthesisResult:
    t0 = time.monotonic()
    payload = [{
        "source_id": r.source_id, "source_type": r.source_type,
        "model": r.model, "confidence": r.confidence,
        "summary": r.summary, "findings": [f.to_dict() for f in r.findings],
    } for r in reviews]

    system = system_template.format(n=len(reviews))
    user_msg = (
        f"## Original Diff (truncated)\n```diff\n{diff[:4000]}\n```\n\n"
        f"## Reviews to Synthesize\n```json\n{json.dumps(payload, indent=2)}\n```")

    call_fn = _LLM_DISPATCH[synth.provider]
    try:
        raw = await call_fn(synth.model, system, user_msg)
        data = _parse_json(raw)
        return SynthesisResult(
            synthesis_id=synthesis_id,
            sources_used=[r.source_id for r in reviews],
            consensus_findings=data.get("consensus_findings", []),
            disputed_findings=data.get("disputed_findings", []),
            summary=data.get("summary", ""),
            recommendation=data.get("recommendation", "needs-discussion"),
            agreement_score=float(data.get("agreement_score", 0.0)),
            latency_ms=int((time.monotonic() - t0) * 1000))
    except Exception as e:
        return SynthesisResult(
            synthesis_id=synthesis_id,
            sources_used=[r.source_id for r in reviews],
            consensus_findings=[], disputed_findings=[],
            summary=f"Synthesis failed: {e}",
            recommendation="needs-discussion", agreement_score=0.0,
            latency_ms=int((time.monotonic() - t0) * 1000))


# ---------------------------------------------------------------------------
# Main orchestrator
# ---------------------------------------------------------------------------

async def run_council(
    diff: str, context: str = "",
    council: list[MemberConfig] | None = None,
    synthesizer: MemberConfig | None = None,
    include_coderabbit: bool = True,
    coderabbit_config: dict | None = None,
    cached_coderabbit: Optional[SourceReview] = None,
) -> CouncilResult:
    t_start = time.monotonic()
    council = council or DEFAULT_COUNCIL
    synthesizer = synthesizer or DEFAULT_SYNTHESIZER
    cr_config = coderabbit_config or {}

    # Filter council to providers that have API keys configured
    active_council = []
    for m in council:
        if _has_key(m.provider):
            active_council.append(m)

    if not active_council:
        return CouncilResult(
            individual_reviews=[], council_synthesis=None, grand_synthesis=None,
            total_latency_ms=0)

    # Fan-out: all LLMs + CodeRabbit in parallel
    tasks = [_review_llm(m, diff, context) for m in active_council]
    if include_coderabbit and cached_coderabbit is None:
        tasks.append(_call_coderabbit(diff, cr_config))

    results = await asyncio.gather(*tasks)

    llm_reviews = [r for r in results if r.source_type == "llm"]
    cr_review = cached_coderabbit or next(
        (r for r in results if r.source_type == "coderabbit"), None)

    all_reviews = list(llm_reviews)
    if cr_review:
        all_reviews.append(cr_review)

    # Council synthesis (LLMs only)
    valid_llm = [r for r in llm_reviews if not r.error and r.confidence > 0]
    council_synth = None
    if len(valid_llm) >= 2:
        council_synth = await _synthesize(
            valid_llm, diff, synthesizer, COUNCIL_SYNTHESIS_SYSTEM, "council-synthesis")

    # Grand synthesis (council + CodeRabbit)
    grand_synth = None
    sources_for_grand = []

    if council_synth:
        council_as_review = SourceReview(
            source_id="council-consensus", source_type="llm", model="council-synthesis",
            findings=[Finding(**{k: f.get(k) for k in
                ["severity", "category", "file", "line", "title", "description", "suggestion"]})
                for f in council_synth.consensus_findings],
            summary=council_synth.summary,
            confidence=council_synth.agreement_score, latency_ms=council_synth.latency_ms)
        sources_for_grand.append(council_as_review)

    if cr_review and not cr_review.error and cr_review.findings:
        sources_for_grand.append(cr_review)

    if len(sources_for_grand) >= 2:
        grand_synth = await _synthesize(
            sources_for_grand, diff, synthesizer, GRAND_SYNTHESIS_SYSTEM, "grand-synthesis")

    return CouncilResult(
        individual_reviews=all_reviews,
        council_synthesis=council_synth,
        grand_synthesis=grand_synth,
        total_latency_ms=int((time.monotonic() - t_start) * 1000))


def _has_key(provider: Provider) -> bool:
    """Check if the required API key for a provider is set."""
    key_map = {
        Provider.ANTHROPIC:  "ANTHROPIC_API_KEY",
        Provider.OPENAI:     "OPENAI_API_KEY",
        Provider.GOOGLE:     "GOOGLE_API_KEY",
        Provider.MINIMAX:    "MINIMAX_API_KEY",
        Provider.MOONSHOT:   "MOONSHOT_API_KEY",
        Provider.OPENROUTER: "OPENROUTER_API_KEY",
    }
    return bool(os.environ.get(key_map.get(provider, ""), ""))


# ---------------------------------------------------------------------------
# Extract findings per source (for benchmarking)
# ---------------------------------------------------------------------------

def extract_findings_by_source(result: CouncilResult) -> dict[str, list[dict]]:
    out = {}
    for r in result.individual_reviews:
        out[r.source_id] = [f.to_dict() for f in r.findings]
    if result.council_synthesis:
        cs = result.council_synthesis
        findings = list(cs.consensus_findings)
        findings.extend(f for f in cs.disputed_findings if f.get("verdict") != "invalid")
        out["council-synthesis"] = findings
    if result.grand_synthesis:
        gs = result.grand_synthesis
        findings = list(gs.consensus_findings)
        findings.extend(f for f in gs.disputed_findings if f.get("verdict") != "invalid")
        out["grand-synthesis"] = findings
    return out


# ---------------------------------------------------------------------------
# Report formatter
# ---------------------------------------------------------------------------

_SEV = {"critical": "🔴", "major": "🟠", "minor": "🟡", "suggestion": "🔵"}

def format_report(result: CouncilResult) -> str:
    lines = ["# 🏛️ LLM Council + CodeRabbit — Code Review Report\n"]

    synth = result.grand_synthesis or result.council_synthesis
    if synth:
        rec = synth.recommendation
        badge = {"approve": "✅ APPROVE", "request-changes": "🚫 REQUEST CHANGES"}.get(
            rec, "💬 NEEDS DISCUSSION")
        lines.append(f"**Recommendation:** {badge}")
        lines.append(f"**Source Agreement:** {synth.agreement_score:.0%}")
        lines.append(f"**Sources Used:** {', '.join(synth.sources_used)}\n")
        lines.append(f"## Summary\n\n{synth.summary}\n")

        if synth.consensus_findings:
            lines.append("## ✅ Consensus Findings\n")
            for f in synth.consensus_findings:
                sev = f.get("severity", "minor")
                loc = f.get("file", "?")
                if f.get("line"): loc += f":{f['line']}"
                sources = f.get("agreed_by", f.get("sources", []))
                lines.append(f"### {_SEV.get(sev, '⚪')} [{sev.upper()}] {f.get('title', 'Issue')}")
                lines.append(f"📍 `{loc}` — Agreed by: {', '.join(sources) if sources else 'multiple'}\n")
                lines.append(f"{f.get('description', '')}\n")
                if f.get("suggestion"):
                    lines.append(f"> 💡 **Fix:** {f['suggestion']}\n")

        if synth.disputed_findings:
            lines.append("## ⚖️ Disputed Findings\n")
            for f in synth.disputed_findings:
                sev = f.get("severity", "minor")
                v = f.get("verdict", "uncertain")
                ve = {"valid": "✅", "invalid": "❌", "uncertain": "❓"}.get(v, "❓")
                loc = f.get("file", "?")
                if f.get("line"): loc += f":{f['line']}"
                src = f.get("raised_by", f.get("source", "?"))
                lines.append(f"### {_SEV.get(sev, '⚪')} [{sev.upper()}] {f.get('title', 'Issue')} — {ve} {v}")
                lines.append(f"📍 `{loc}` — Raised by: {src}\n")
                lines.append(f"{f.get('description', '')}\n")
                if f.get("reasoning"):
                    lines.append(f"> **Verdict reasoning:** {f['reasoning']}\n")

    # Reviewer details
    lines.append("## 📊 Reviewer Details\n")
    lines.append("| Source | Type | Model | Findings | Confidence | Latency | Status |")
    lines.append("|--------|------|-------|----------|------------|---------|--------|")
    for r in result.individual_reviews:
        status = f"⚠️ {r.error[:40]}" if r.error else "✅"
        lines.append(
            f"| {r.source_id} | {r.source_type} | {r.model} | "
            f"{len(r.findings)} | {r.confidence:.0%} | {r.latency_ms}ms | {status} |")
    if result.council_synthesis:
        cs = result.council_synthesis
        n = len(cs.consensus_findings) + len(cs.disputed_findings)
        lines.append(f"| council-synthesis | synthesis | — | {n} | {cs.agreement_score:.0%} | {cs.latency_ms}ms | ✅ |")
    if result.grand_synthesis:
        gs = result.grand_synthesis
        n = len(gs.consensus_findings) + len(gs.disputed_findings)
        lines.append(f"| grand-synthesis | synthesis | — | {n} | {gs.agreement_score:.0%} | {gs.latency_ms}ms | ✅ |")
    lines.append(f"\n**Total pipeline latency:** {result.total_latency_ms}ms")

    return "\n".join(lines)


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

async def main():
    import argparse, sys

    parser = argparse.ArgumentParser(description="LLM Council + CodeRabbit review")
    parser.add_argument("--diff-file", "-d", help="Diff file (or stdin)")
    parser.add_argument("--context", "-c", default="", help="Project context")
    parser.add_argument("--output", "-o", help="Output file")
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--no-coderabbit", action="store_true")
    parser.add_argument("--models", nargs="*",
        help="Override: 'anthropic:model openai:model minimax:model moonshot:model'")
    args = parser.parse_args()

    if args.diff_file:
        diff = Path(args.diff_file).read_text()
    elif not sys.stdin.isatty():
        diff = sys.stdin.read()
    else:
        print("Provide diff via --diff-file or stdin", file=sys.stderr); sys.exit(1)

    council = None
    if args.models:
        council = []
        for spec in args.models:
            p, m = spec.split(":", 1)
            council.append(MemberConfig(f"{p}:{m}", Provider(p), m))

    result = await run_council(
        diff, context=args.context, council=council,
        include_coderabbit=not args.no_coderabbit)

    if args.json:
        sources = extract_findings_by_source(result)
        output = json.dumps({
            "sources": sources,
            "recommendation": getattr(result.grand_synthesis or result.council_synthesis,
                                       "recommendation", "unknown"),
            "total_latency_ms": result.total_latency_ms,
        }, indent=2)
    else:
        output = format_report(result)

    if args.output:
        Path(args.output).parent.mkdir(parents=True, exist_ok=True)
        Path(args.output).write_text(output)
        print(f"Written to {args.output}", file=sys.stderr)
    else:
        print(output)


if __name__ == "__main__":
    asyncio.run(main())
