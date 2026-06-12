---
name: modular-services
description: >-
  Structure any coding task as small, single-responsibility "service modules" —
  each with an explicit typed input contract, an explicit typed output contract,
  and exactly one public entrypoint — so a failure stays localized to one unit
  and the data flow can be read and diagrammed at a glance. Language-agnostic
  (Python, TypeScript, Rust, C++, Go, …). Use whenever writing a new feature,
  adding a module/function of any real size, or refactoring tangled code into
  clear units, or when the user asks for "microservice-style", modular, or
  contract-first code.
---

# Modular Services (typed input → typed output units)

Write code as a set of small, **micro-service-style units**: each does one job,
takes a single explicitly-typed **input**, returns a single explicitly-typed
**output**, and exposes **one public entrypoint**. Everything else in the unit
is private.

This is *not* about deploying separate processes. A "service" here is a code
unit (a function, module, or class) with a hard contract at its edges — the
same discipline a network microservice has, without the network.

## Why this is the rule

- **Errors localize.** When every unit has a typed boundary, a wrong value is
  caught at the edge of the unit that produced it — not three layers deep where
  the symptom shows up. You can point at one unit and one contract.
- **Code reads as a flow.** Units + their input/output types form a directed
  graph: `A(in→out) → B(in→out) → C`. Anyone (or any diagram tool) can trace
  the data path without reading the bodies.
- **Each unit is testable in isolation.** Construct the input, call the
  entrypoint, assert the output. No mocking the world.

## Anatomy of a unit

Every unit you write MUST have all five:

1. **One responsibility** — describable in a single sentence with no "and".
   "Size a position from equity and stop distance." If you need "and", split it.
2. **One input contract** — a single named, typed value carrying *everything*
   the unit needs. No grab-bag of loose positional args; no reading globals.
3. **One output contract** — a single named, typed value. The caller never has
   to guess the shape of what comes back.
4. **One public entrypoint** — a single verb-named function/method. All helpers
   are private (module-private, `_prefixed`, `static`, not exported, etc.).
5. **Errors in the contract** — failure modes are *part of the output type*
   (a result/verdict variant) or a documented typed error — never a silent
   `None`/`null`/`-1`/swallowed exception.

Dependencies (clients, clocks, db handles) come **in** — through the input or a
constructor — so the unit stays pure-ish and testable. Don't reach for module
globals or singletons inside a unit.

## Workflow when you code

1. **Decompose first.** Before writing bodies, list the units the task needs and
   the data that flows between them. One line each: `name: InType → OutType`.
2. **Define the contracts before the logic.** Write the input and output types
   first. They are the spec; the body just has to satisfy them.
3. **Implement behind the entrypoint.** Fill in one unit at a time. Keep helpers
   private to that unit.
4. **Wire units together** by passing one unit's output into the next's input.
   The wiring layer (a thin orchestrator/route handler) does no business logic —
   it only connects contracts.
5. **Sanity-check the boundaries.** For each unit ask: could I test this by
   building its input and asserting its output, with nothing else mocked? If
   not, the contract is leaking — fix it.

## Contract patterns by language

The shape is identical everywhere: a struct/type for **in**, a struct/type for
**out**, one entrypoint.

**Python** (use Pydantic where models already exist, else `@dataclass`):

```python
# position_sizer.py
from decimal import Decimal
from pydantic import BaseModel

class SizingRequest(BaseModel):          # INPUT contract
    equity: Decimal
    risk_fraction: Decimal
    entry_price: Decimal
    stop_price: Decimal

class SizingResult(BaseModel):           # OUTPUT contract
    quantity: Decimal
    notional: Decimal

def size_position(req: SizingRequest) -> SizingResult:   # the one entrypoint
    risk_per_unit = abs(req.entry_price - req.stop_price)
    quantity = (req.equity * req.risk_fraction) / risk_per_unit
    return SizingResult(quantity=quantity, notional=quantity * req.entry_price)
```

**TypeScript**:

```typescript
// positionSizer.ts
export interface SizingRequest {         // INPUT contract
  equity: number;
  riskFraction: number;
  entryPrice: number;
  stopPrice: number;
}
export interface SizingResult {          // OUTPUT contract
  quantity: number;
  notional: number;
}
export function sizePosition(req: SizingRequest): SizingResult {   // entrypoint
  const riskPerUnit = Math.abs(req.entryPrice - req.stopPrice);
  const quantity = (req.equity * req.riskFraction) / riskPerUnit;
  return { quantity, notional: quantity * req.entryPrice };
}
```

**Rust** — the contract idea is native (`struct` in, `struct`/`Result` out):

```rust
pub struct SizingRequest { pub equity: f64, pub risk_fraction: f64,
                           pub entry_price: f64, pub stop_price: f64 }
pub struct SizingResult  { pub quantity: f64, pub notional: f64 }

pub fn size_position(req: &SizingRequest) -> SizingResult { /* ... */ }
```

**C++** (header declares the contract; helpers stay in the `.cpp`):

```cpp
struct SizingRequest { double equity, risk_fraction, entry_price, stop_price; };
struct SizingResult  { double quantity, notional; };
SizingResult size_position(const SizingRequest& req);
```

**Go** — the idiomatic `(Result, error)` *is* the error-in-contract pattern:

```go
type SizingRequest struct { Equity, RiskFraction, EntryPrice, StopPrice float64 }
type SizingResult  struct { Quantity, Notional float64 }
func SizePosition(req SizingRequest) (SizingResult, error) { /* ... */ }
```

## Errors are part of the output contract

Make failure a value the caller can see and branch on, not a surprise.

```typescript
// Discriminated result — the type tells the caller every outcome up front
export type RiskVerdict =
  | { ok: true;  order: PreparedOrder }
  | { ok: false; reason: "exceeds_budget" | "outside_hours" | "stale_price" };
```

```rust
pub enum RiskVerdict { Approved(PreparedOrder), Rejected(RejectReason) }
```

```python
class RiskVerdict(BaseModel):
    approved: bool
    reason: RejectReason | None = None     # set iff approved is False
```

If a unit can fail, its output type says so. No `return None` to mean "couldn't",
no `-1` sentinels, no exceptions thrown for ordinary control flow.

## Granularity — don't over- or under-split

- A unit is a **meaningful step in the flow**, not a single line. One unit per
  line is noise; the goal is a readable graph, not maximal fragmentation.
- If a unit's input type has ~6+ unrelated fields or the body has clearly
  separable phases, it's doing more than one job — split it.
- If two "units" are always called together and never independently, and neither
  is independently testable, they're one unit — merge them.
- Pure transformation logic gets its own unit; I/O (db, network, files) lives in
  thin adapter units at the edges, kept out of the logic units.

## Anti-patterns this skill forbids

- A function taking 7 loose positional/keyword args instead of one input type.
- Returning untyped bags: `dict[str, Any]`, `object`, `any`, `void*`,
  `interface{}` as the public output.
- A "unit" that reads/writes module globals or singletons instead of taking its
  dependencies as input.
- Multiple public entrypoints into one unit that each do a different job.
- Business logic living in the wiring layer (route handler, `main`, controller).
- Signalling failure by `None`/`null`/`-1`/empty-string sentinels or by throwing
  on the normal path.

## Checklist before you call a unit done

- [ ] Its job fits in one sentence with no "and".
- [ ] It takes exactly one explicitly-typed input value.
- [ ] It returns exactly one explicitly-typed output value.
- [ ] It has exactly one public entrypoint; helpers are private.
- [ ] Every failure mode is visible in the output type (or a documented typed error).
- [ ] Dependencies are passed in, not reached for.
- [ ] You could test it by building the input and asserting the output, nothing else mocked.
