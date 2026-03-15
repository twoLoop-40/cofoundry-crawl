---
name: visual-qa-click
description: "페이지의 버튼/링크를 클릭하고 전후 스크린샷을 찍어 인터랙션 흐름을 분석하는 시각적 QA 스킬. `/visual-qa-click <url>` 으로 실행."
user-invokable: true
---

# Visual QA Click — 클릭 인터랙션 분석

> 버튼, 링크, 네비게이션 등 클릭 가능한 요소를 자동으로 찾아 클릭하고,
> 전후 스크린샷 + URL 변화 + DOM 변화를 기록하여 AI가 분석한다.

## 트리거

- `/visual-qa-click <url>` — URL의 클릭 가능 요소 자동 탐색 + 분석
- `/visual-qa-click <url> --targets="selector1,selector2"` — 특정 셀렉터만 클릭
- `/visual-qa-click run` — 이미 촬영된 output 폴더를 AI 분석만 실행

## 사용 예시

```
/visual-qa-click http://127.0.0.1:5173/
/visual-qa-click http://127.0.0.1:5173/domains --targets="[data-testid='domain-row'],[data-testid='col-findings'] button"
/visual-qa-click https://safe-intelligence.vercel.app/ --auth --depth=2
```

---

## 사전 조건

`tools/visual-qa/` 없으면 `visual-qa` 스킬의 "사전 조건: 크롤러 자동 생성" 섹션을 먼저 실행.
소스는 `~/.claude/skills/visual-qa/sources/`에 포함되어 있음.

## Phase 1: 크롤 + 클릭 촬영

### 크롤러 실행

```bash
cd tools/visual-qa
npx tsx src/click-crawl.ts <url> [options]
```

### 옵션

| 옵션 | 기본값 | 설명 |
|------|--------|------|
| `--output=DIR` | `./output-click` | 결과 디렉토리 |
| `--targets=SELECTORS` | 자동 탐색 | 클릭할 CSS 셀렉터 (콤마 구분) |
| `--depth=N` | 1 | 클릭 깊이 (1=현재 페이지만, 2=이동한 페이지에서도 클릭) |
| `--max-clicks=N` | 30 | 최대 클릭 횟수 |
| `--viewport-width=N` | 1440 | 뷰포트 너비 |
| `--viewport-height=N` | 900 | 뷰포트 높이 |
| `--wait=N` | 3000 | 클릭 후 대기 (ms) |
| `--auth` | false | 로그인 필요 시 |
| `--auth-email=STR` | - | 로그인 이메일 |
| `--auth-password=STR` | - | 로그인 비밀번호 |
| `--login-url=STR` | `<origin>/login` | 로그인 페이지 URL |
| `--no-navigate` | false | 페이지 이동 없이 인라인 변화만 추적 |
| `--restore` | true | 각 클릭 후 원래 페이지로 복원 |

### 자동 탐색 대상 (--targets 미지정 시)

우선순위 순서:

1. `[data-testid]` 속성이 있는 버튼/링크
2. `nav` 안의 `a`, `button`
3. 테이블 행 (`tr[data-testid]`, `tbody tr`)
4. `button[aria-expanded]` (토글/확장)
5. 카드 클릭 영역 (`[role="button"]`, `.cursor-pointer`)
6. 일반 `a[href]` (외부 링크 제외)

### 출력물

```
output-click/
├── screenshots/
│   ├── 00-initial.png                    # 초기 상태
│   ├── 01-before-nav-domains.png         # 클릭 전
│   ├── 01-after-nav-domains.png          # 클릭 후
│   ├── 02-before-domain-row-0.png
│   ├── 02-after-domain-row-0.png
│   └── ...
├── click-results.json                    # 전체 결과
└── click-meta.json                       # 메타데이터
```

### click-results.json 구조

```json
{
  "url": "http://127.0.0.1:5173/",
  "timestamp": "2026-03-11T...",
  "totalClicks": 15,
  "clicks": [
    {
      "index": 0,
      "selector": "nav a[href='/domains']",
      "label": "도메인",
      "type": "navigation",
      "before": {
        "url": "http://127.0.0.1:5173/",
        "screenshot": "01-before-nav-domains.png"
      },
      "after": {
        "url": "http://127.0.0.1:5173/domains",
        "screenshot": "01-after-nav-domains.png",
        "urlChanged": true,
        "newElements": ["table", "[data-testid='domain-row']"],
        "removedElements": ["[data-testid='section-01']"]
      },
      "params": { "domain_id": null, "scope_type": null },
      "duration": 1200
    },
    {
      "index": 1,
      "selector": "[data-testid='domain-row']:nth-child(1)",
      "label": "jiran.com",
      "type": "expand",
      "before": { "ariaExpanded": "false", "screenshot": "..." },
      "after": {
        "ariaExpanded": "true",
        "screenshot": "...",
        "urlChanged": false,
        "newElements": ["[data-testid='subdomain-row']"]
      }
    }
  ]
}
```

---

## Phase 2: AI 분석

### Step 1: 결과 로드

`click-results.json` 읽기 → 클릭 수, 유형 분류

### Step 2: 클릭별 전후 비교

각 클릭의 before/after 스크린샷 쌍을 Read tool로 열어서 비교:

| 분석 항목 | 추출 |
|-----------|------|
| **네비게이션 일관성** | URL 파라미터가 올바르게 전달되는지 (domain_id, scope_type 등) |
| **상태 전이** | aria-expanded 변화, 새로 나타난 요소, 사라진 요소 |
| **데이터 일관성** | 클릭 전 숫자 → 클릭 후 페이지의 숫자가 일치하는지 |
| **시각적 피드백** | hover 효과, 로딩 상태, 에러 상태 |
| **빈 상태** | 클릭 후 빈 페이지, 에러 메시지 |

### Step 3: 패턴 분석

```
## 클릭 분석: http://127.0.0.1:5173/

### 네비게이션 흐름
- 사이드바 메뉴: 7개 링크 모두 정상 이동 ✅
- 도메인 카드 → 도메인 상세: domain_id 파라미터 보존 ✅
- 파인딩 카운트 → /findings: scope_type=domain 전달 ✅
- 다크웹 카운트 → /findings: finding_type=dark_web_mention ❌ (scope 누락)

### 확장/토글
- CollapsibleSection: 3개 모두 열림/닫힘 정상 ✅
- 도메인 행 아코디언: 서브도메인 목록 로드 ✅
- ExpandableItem: aria-expanded 전이 정상 ✅

### 데이터 일관성
- 도메인 목록 finding count (41) == 도메인 상세 count (41) ✅
- 서브도메인 목록 count (23) ≠ 상세 count (21) ❌

### 문제점
1. [NAV] 다크웹 카운트 클릭 시 scope_type 미전달
2. [DATA] 서브도메인 카운트 불일치 (23 vs 21)
3. [UX] 도메인 카드의 grade 뱃지 클릭 불가 (영역 너무 작음)
```

### Step 4: 깊이 2 모드 (--depth=2)

depth=2일 때, 이동한 페이지에서 한 번 더 클릭 탐색:

```
[Page 1] / → click "도메인" → [Page 2] /domains
  → [Page 2] /domains → click "jiran.com row" → [Page 3] /domains/123
    → [Page 3] 결과 분석
  → [Page 2] /domains → click "findings count" → [Page 3] /findings?domain_id=123
    → [Page 3] 결과 분석
```

---

## Phase 3: Lean 4 DAG 타입시스템 생성

> **공통 규칙**: `~/.claude/skills/visual-qa-types/SKILL.md` 참조.
> L1 공통 타입은 `Specs/UI/VisualQATypes.lean`, 페이지별 L2/L3는 이 스킬이 생성.

### Click가 생성하는 Lean 파일

| Layer | 파일 | 내용 |
|-------|------|------|
| L2 | `Specs/UI/<Page>Interactions.lean` | 클릭 대상 열거 + 전이 목록 + scope 보존 검증 |
| L3 | `Specs/UI/<Page>FlowVerify.lean` | 네비게이션 일관성 + 데이터 일관성 theorem |

### 매핑 테이블

| 크롤링 데이터 | Lean 4 패턴 | Layer |
|-------------|-------------|-------|
| 클릭 전후 상태 | `ClickCapture` + `StateTransition` (L1 공통) | L1 |
| 네비게이션 흐름 | `def navTransitions` + scope 보존 | L2 Interactions |
| aria-expanded 전이 | `def expandTransitions` | L2 Interactions |
| URL 파라미터 보존 | `theorem scopeAlwaysPreserved` | L2 Interactions |
| 데이터 일관성 | `DataConsistency` + `FlowVerifyResult` | L3 FlowVerify |
| 전체 pass/fail | `theorem noDataMismatch` | L3 FlowVerify |

### 이후 워크플로우

- `spec-qa-verify` → 구현 수정 → QA 사이클로 이어감
- 네비게이션 불일치는 `goal-driven-dev` 갭 리포트에 연결
- 스코프 보존 실패는 즉시 버그픽스 대상

---

## 크롤러 없으면 설치

```bash
cd tools/visual-qa && npm install && npx playwright install chromium
```

## 기존 visual-qa 스킬과의 관계

| 스킬 | 목적 | 핵심 차이 |
|------|------|-----------|
| **visual-qa** | BFS 전체 사이트 크롤 + Spec 생성 | 링크 따라가며 전체 탐색, Lean 4 Spec 출력 |
| **visual-qa-mosaic** | 단일 페이지 타일 분석 | 한 페이지를 깊이 있게 레이아웃 분석 |
| **visual-qa-click** | 클릭 인터랙션 분석 | 버튼/링크 클릭 전후 변화 추적, 네비 일관성 검증 |

## 크롤러 위치

`tools/visual-qa/src/click-crawl.ts`
