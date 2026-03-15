---
name: visual-qa-types
description: "visual-qa 계열 스킬(visual-qa, visual-qa-mosaic, visual-qa-click)이 공유하는 Lean 4 DAG 타입시스템 생성 규칙. 직접 호출하지 않고, 다른 스킬에서 참조."
user-invokable: false
---

# Visual QA Types — 공유 DAG 타입시스템

> **이 스킬은 직접 호출하지 않는다.**
> `visual-qa`, `visual-qa-mosaic`, `visual-qa-click`이 크롤링 후 타입시스템을 생성할 때 이 규칙을 따른다.

> **⚠️ DAG-First 규칙 필수**: `~/.claude/skills/lean-dag-rules/SKILL.md` 참조.
> import ≥ 1, canonical 타입 사용, sorry 금지.

---

## 핵심 원칙

1. **단일 Spec이 아닌 import DAG** — 크롤링 결과를 3-Layer DAG로 변환
2. **기존 트리에 통합** — `Specs/Common/Types.lean`을 import 원점으로 연결
3. **Poincaré 임베딩 등록** — 생성 후 반드시 `extract_pairs + train` 실행
4. **Total function** — 모든 매핑 함수는 빠뜨린 case가 있으면 컴파일 에러
5. **Theorem 카운트** — 열거형 크기를 `native_decide`로 강제 추적

---

## 3-Layer DAG 아키텍처

```
Specs/Common/Types.lean                      ← 기존 (공통 타입, import 원점)
    ↑
Specs/UI/VisualQATypes.lean                  ← L1: 공통 원시 타입 (3 스킬 공유)
    ↑                    ↑                ↑
    │                    │                │
Specs/UI/<Page>Layout.lean                   ← L2-mosaic: 페이지 섹션 구조
Specs/UI/<Page>Interactions.lean             ← L2-click: 클릭 인터랙션 흐름
Specs/UI/<Page>RouteSpec.lean                ← L2-visual-qa: 라우트+API 구조
    ↑                    ↑                ↑
    │                    │                │
Specs/UI/<Page>Alignment.lean                ← L3: 비교/정렬/갭 결과
Specs/UI/<Page>FlowVerify.lean               ← L3: 네비게이션 일관성 검증
```

---

## Layer 1: 공통 원시 타입 (VisualQATypes.lean)

3개 스킬이 모두 import하는 공유 기반 타입.
**한 번만 만들고 재사용.**

```lean
import Specs.Common.Types

set_option autoImplicit true

namespace Specs.UI.VisualQATypes

-- ══════════════════════════════════════
-- 레이아웃 타입 (mosaic에서 주로 사용)
-- ══════════════════════════════════════

/-- 페이지 섹션 역할 --/
inductive SectionRole where
  | header | hero | content | sidebar | footer | nav | modal
  deriving BEq, DecidableEq, Repr

/-- UI 컴포넌트 카테고리 --/
inductive ComponentKind where
  | card | table | badge | chart | button | input | grid | list
  | drawer | modal | tab | accordion | form
  deriving BEq, DecidableEq, Repr

/-- 레이아웃 방식 --/
inductive LayoutMode where
  | singleColumn | twoColumn | grid (cols : Nat) | flex | stack
  deriving BEq, Repr

-- ══════════════════════════════════════
-- 인터랙션 타입 (click에서 주로 사용)
-- ══════════════════════════════════════

/-- 클릭 대상 유형 --/
inductive ClickTargetKind where
  | navLink | button | tableRow | accordion | toggle | cardClick | formSubmit
  deriving BEq, DecidableEq, Repr

/-- 클릭 결과 유형 --/
inductive ClickEffect where
  | navigate (path : String)
  | expand
  | collapse
  | openModal
  | closeModal
  | submitForm
  | noChange
  deriving BEq, Repr

/-- 상태 전이 --/
structure StateTransition where
  fromUrl : String
  toUrl : String
  clickTarget : ClickTargetKind
  effect : ClickEffect
  urlChanged : Bool

-- ══════════════════════════════════════
-- 비교/정렬 타입 (모든 스킬에서 사용)
-- ══════════════════════════════════════

/-- Figma ↔ 구현 정렬 상태 --/
inductive Alignment where
  | matched | mismatch (detail : String) | missing | extra
  deriving BEq, Repr

/-- 데이터 일관성 --/
inductive DataConsistency where
  | consistent | mismatch (expected : String) (actual : String) | notChecked
  deriving BEq, Repr

-- ══════════════════════════════════════
-- 라우트/API 타입 (visual-qa에서 주로 사용)
-- ══════════════════════════════════════

/-- 라우트 보호 수준 --/
inductive RouteProtection where
  | public_ | authenticated | adminOnly
  deriving BEq, DecidableEq, Repr

/-- API 엔드포인트 메타 --/
structure EndpointMeta where
  method : String        -- GET, POST, etc.
  path : String
  requiresAuth : Bool
  hasResponse : Bool

-- ══════════════════════════════════════
-- 타일/스크린샷 메타
-- ══════════════════════════════════════

/-- 모자이크 타일 --/
structure TileMeta where
  index : Nat
  yOffset : Nat
  height : Nat
  sectionRoles : List SectionRole

/-- 클릭 전후 스크린샷 쌍 --/
structure ClickCapture where
  index : Nat
  selector : String
  label : String
  targetKind : ClickTargetKind
  beforeScreenshot : String
  afterScreenshot : String
  transition : StateTransition

end Specs.UI.VisualQATypes
```

---

## Layer 2: 스킬별 구체 타입

### visual-qa-mosaic → `<Page>Layout.lean`

```lean
import Specs.UI.VisualQATypes

namespace Specs.UI.OverviewLayout  -- 페이지별 네이밍

open VisualQATypes

inductive Section where
  | header | immediateActions | assetExposure | threatIntelligence | footer
  deriving BEq, DecidableEq, Repr

def sectionOrder : List Section := [.header, .immediateActions, ...]
def sectionRole : Section → SectionRole  -- total function
def sectionComponents : Section → List ComponentKind  -- total function

theorem sectionCount : sectionOrder.length = N := by native_decide
```

### visual-qa-click → `<Page>Interactions.lean`

```lean
import Specs.UI.VisualQATypes

namespace Specs.UI.OverviewInteractions

open VisualQATypes

/-- 이 페이지의 클릭 가능 요소 --/
def clickTargets : List ClickCapture := [...]

/-- 네비게이션 일관성: 모든 nav 링크가 올바른 페이지로 이동 --/
def navTransitions : List StateTransition := [...]

/-- 스코프 보존: domain_id 파라미터가 전달되는지 --/
def scopePreservingClicks : List StateTransition :=
  navTransitions.filter (fun t => t.urlChanged && ...)

theorem allNavLinksNavigate : navTransitions.all (fun t => t.urlChanged) = true := by native_decide
theorem scopeAlwaysPreserved : scopePreservingClicks.length = N := by native_decide
```

### visual-qa → `<Page>RouteSpec.lean`

```lean
import Specs.UI.VisualQATypes

namespace Specs.UI.AppRouteSpec

open VisualQATypes

/-- 전체 라우트 목록 --/
inductive Route where
  | overview | domains | domainDetail | findings | findingDetail
  | darkWeb | intelligence | pentest | riskAnalysis | settings | login
  deriving BEq, DecidableEq, Repr

def routeProtection : Route → RouteProtection  -- total function
def routeEndpoints : Route → List EndpointMeta  -- total function

theorem protectedRouteCount : (allRoutes.filter (·.protection != .public_)).length = N := by native_decide
theorem totalEndpoints : (allRoutes.bind routeEndpoints).length = M := by native_decide
```

---

## Layer 3: 비교/검증 타입

### 디자인 정렬 — `<Page>Alignment.lean`

```lean
import Specs.UI.OverviewLayout  -- L2 import

structure AlignmentItem where
  component : ComponentKind
  status : Alignment
  figmaDetail : String := ""
  implDetail : String := ""

structure AlignmentReport where
  items : List AlignmentItem
  matchedCount : Nat
  gapCount : Nat
  -- theorem: matchedCount + gapCount = items.length
```

### 네비게이션 검증 — `<Page>FlowVerify.lean`

```lean
import Specs.UI.OverviewInteractions  -- L2 import

structure FlowVerifyResult where
  transitions : List StateTransition
  consistencyChecks : List DataConsistency
  passCount : Nat
  failCount : Nat

theorem noDataMismatch : result.failCount = 0 := by native_decide
```

---

## 크롤링 데이터 → Lean 4 매핑 (통합)

| 크롤링 데이터 | Lean 4 패턴 | 원본 스킬 | DAG Layer |
|-------------|-------------|-----------|-----------|
| 타일별 섹션 | `TileMeta` + `SectionRole` | mosaic | L1 |
| 컴포넌트 목록 | `ComponentKind` + `sectionComponents` | mosaic | L2 |
| 클릭 전후 상태 | `ClickCapture` + `StateTransition` | click | L1 |
| 네비게이션 흐름 | `navTransitions` + scope 보존 | click | L2 |
| URL+라우트 목록 | `inductive Route` + `routeProtection` | visual-qa | L2 |
| API 엔드포인트 | `EndpointMeta` + `routeEndpoints` | visual-qa | L2 |
| 폼 필드+유효성 | `structure FormField` + 의존타입 | visual-qa | L2 |
| Figma 비교 | `AlignmentReport` | mosaic | L3 |
| 데이터 일관성 | `DataConsistency` + `FlowVerifyResult` | click | L3 |

---

## 생성 후 필수 작업

### 1. lakefile 등록

```bash
# 새 Lean 파일을 lakefile.lean에 추가
# @[default_target] lean_lib Specs where ...
```

### 2. 빌드 검증

```bash
lake build Specs.UI.VisualQATypes
lake build Specs.UI.OverviewLayout  # 페이지별
```

### 3. Poincaré 임베딩 갱신 (필수)

```bash
python3 tools/poincare-specs/extract_pairs.py
python3 tools/poincare-specs/train.py

# 검증
python3 tools/poincare-specs/search.py "visual qa layout"
python3 tools/poincare-specs/search.py "click interaction"
```

---

## Lean 4 프라이밍 (공통)

- `import` 최상단, `set_option autoImplicit true`
- 콜론 공백: `field : Type`, 키워드 회피 (`meta` → `sessionMeta`, `partial` → `partial_`)
- `deriving BEq, DecidableEq` — `by decide` / `by native_decide` 전제조건
- sorry 금지: `by omega`, `by native_decide`, `axiom` 사용
- 키워드 `public` 사용 금지 → `public_` 사용

---

## 스킬별 참조 방법

각 스킬의 Phase 3 (타입시스템 생성) 섹션에서:

```
## Phase 3: Lean 4 DAG 타입시스템 생성

> **공통 규칙**: `~/.claude/skills/visual-qa-types/SKILL.md` 참조.
> L1 공통 타입은 `Specs/UI/VisualQATypes.lean`, 페이지별 L2/L3는 스킬이 생성.
```
