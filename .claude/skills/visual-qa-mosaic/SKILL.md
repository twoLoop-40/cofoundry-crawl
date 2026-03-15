---
name: visual-qa-mosaic
description: "페이지를 타일 단위로 분할 촬영하여 전체 레이아웃을 분석하는 시각적 QA 스킬. `/visual-qa-mosaic <url>` 으로 크롤+분석 한 번에 실행."
user-invokable: true
---

# Visual QA Mosaic — 전체 페이지 타일 분석

> 페이지 하나를 여러 장의 타일로 나눠 찍고, AI가 전체 레이아웃을 분석한다.
> Figma 디자인 검토, 페이지 레이아웃 감사, 디자인 vs 구현 비교에 최적.

## 트리거

- `/visual-qa-mosaic <url>` — URL을 크롤+타일 촬영+AI 분석
- `/visual-qa-mosaic <url> --compare <url2>` — 두 URL을 비교 분석
- `/visual-qa-mosaic run` — 이미 촬영된 output 폴더의 타일을 AI 분석만 실행

## 사용 예시

```
/visual-qa-mosaic https://revamp-bonus-20194455.figma.site/
/visual-qa-mosaic http://127.0.0.1:5173/ --auth
/visual-qa-mosaic https://figma.site/design --compare http://127.0.0.1:5173/
```

---

## 사전 조건

`tools/visual-qa/` 없으면 `visual-qa` 스킬의 "사전 조건: 크롤러 자동 생성" 섹션을 먼저 실행.
소스는 `~/.claude/skills/visual-qa/sources/`에 포함되어 있음.

## Phase 1: 크롤 + 타일 촬영

### 크롤러 실행

```bash
cd tools/visual-qa
npx tsx src/mosaic-crawl.ts <url> [options]
```

### 옵션

| 옵션 | 기본값 | 설명 |
|------|--------|------|
| `--output=DIR` | `./output-mosaic` | 결과 디렉토리 |
| `--tile-height=N` | 720 | 타일 높이 (px) |
| `--viewport-width=N` | 1440 | 뷰포트 너비 |
| `--viewport-height=N` | 900 | 뷰포트 높이 |
| `--wait=N` | 4000 | 페이지 로드 후 대기 (ms) |
| `--scroll-wait=N` | 400 | 스크롤 후 대기 (ms) |
| `--auth` | false | 로그인 필요 시 (SPA 인증) |
| `--auth-email=STR` | - | 로그인 이메일 |
| `--auth-password=STR` | - | 로그인 비밀번호 |
| `--login-url=STR` | `<origin>/login` | 로그인 페이지 URL |
| `--pages=URL1,URL2,...` | - | 여러 페이지 순회 (콤마 구분) |

### 출력물

```
output-mosaic/
├── screenshots/
│   ├── 00-full-page.png         # 전체 페이지 (full-page)
│   ├── tile-00.png              # 첫 번째 타일 (0~720px)
│   ├── tile-01.png              # 두 번째 타일 (720~1440px)
│   ├── tile-02.png              # ...
│   └── ...
├── text-full.txt                # 페이지 전체 텍스트 (innerText)
├── dom-structure.json           # heading, section, nav 구조
└── mosaic-meta.json             # 메타데이터 (URL, 타일 수, 높이, 뷰포트)
```

### 여러 페이지 모드

`--pages` 옵션 사용 시 각 페이지별 서브 디렉토리 생성:

```
output-mosaic/
├── page-0-overview/
│   ├── screenshots/tile-00.png ...
│   ├── text-full.txt
│   └── mosaic-meta.json
├── page-1-domains/
│   └── ...
└── summary.json                 # 전체 페이지 요약
```

---

## Phase 2: AI 분석

크롤 결과를 AI가 분석. 수동 트리거: `/visual-qa-mosaic run`

### Step 1: 타일 로드

`mosaic-meta.json` 읽기 → 타일 수, 페이지 높이, URL 확인.

### Step 2: 타일별 분석

각 타일 스크린샷을 Read tool로 읽고 분석:

| 분석 항목 | 추출 |
|-----------|------|
| **섹션 구조** | 어떤 섹션이 보이는지 (header, section-01, footer 등) |
| **컴포넌트 식별** | 카드, 테이블, 배지, 차트, 버튼 등 UI 패턴 |
| **레이아웃 패턴** | grid, flex, 단일 컬럼 등 배치 방식 |
| **색상/타이포** | 주요 색상, 텍스트 크기/무게 |
| **데이터 밀도** | 숫자, 라벨, 빈 공간 비율 |

### Step 3: 전체 종합

타일별 분석을 합쳐 전체 페이지 구조를 리포트:

```
## 페이지 분석: https://example.com/

### 구조 (위→아래)
- Header: 로고 + 네비게이션 + 사용자 메뉴
- Section 01: Immediate Actions (TOP 5 테이블)
- Section 02: Asset Exposure (도메인 카드 그리드)
- Section 03: Threat Intelligence (DW KPI + AI 인사이트)
- Footer: 버전 + 소스

### 컴포넌트 목록
- [Card] 4개: stat-card, domain-card, ai-insight, dw-summary
- [Table] 1개: TOP 5 findings
- [Badge] severity (CRITICAL/HIGH/MEDIUM/LOW)
- [Button] 3개: 전체 보기 링크

### 디자인 패턴
- 색상: navy-950 배경, severity accent
- 간격: section 간 mt-10
- 카드: rounded-card, border-navy-700
```

### Step 4: 비교 모드 (--compare)

두 URL을 촬영한 경우 타일 간 diff 분석:

```
## 비교: Figma vs Implementation

### 일치
- 3-섹션 허브 구조 ✅
- TOP 5 테이블 레이아웃 ✅
- 도메인 카드 그리드 (3열) ✅

### 불일치
- [Section 02] Figma: grade circle 크기 40px → 구현: 32px
- [Section 03] Figma: DW KPI 4열 → 구현: 2열 (반응형 차이)
- [Header] Figma: ACTIVE SCOPE 라벨 → 구현: 누락

### 권장 조치
1. Grade circle 크기를 size-10으로 변경
2. DW KPI grid를 lg:grid-cols-4로 변경
3. ACTIVE SCOPE 라벨 추가
```

---

## Phase 3: Lean 4 DAG 타입시스템 생성

> **공통 규칙**: `~/.claude/skills/visual-qa-types/SKILL.md` 참조.
> L1 공통 타입은 `Specs/UI/VisualQATypes.lean`, 페이지별 L2/L3는 이 스킬이 생성.

### Mosaic가 생성하는 Lean 파일

| Layer | 파일 | 내용 |
|-------|------|------|
| L2 | `Specs/UI/<Page>Layout.lean` | 섹션 순서 + 역할 매핑 + 카운트 theorem |
| L2 | `Specs/UI/<Page>Components.lean` | 컴포넌트 열거 + 섹션별 매핑 (total function) |
| L3 | `Specs/UI/<Page>Alignment.lean` | 비교 모드 시 Figma vs 구현 정렬 결과 |

### 매핑 테이블

| 크롤링 데이터 | Lean 4 패턴 | Layer |
|-------------|-------------|-------|
| 섹션 목록 (위→아래) | `inductive Section` + `def sectionOrder` | L2 Layout |
| 컴포넌트 인벤토리 | `inductive Component` + `def sectionComponents` | L2 Components |
| 타일별 콘텐츠 | `TileMeta` (L1 공통) | L1 |
| 디자인 패턴 | `def tokenUsage` (design-spec-build 연동) | L2 |
| 비교 결과 (diff) | `AlignmentReport` | L3 |

### 이후 워크플로우

- `spec-qa-verify` → 구현 → QA 사이클로 이어감
- 비교 모드의 불일치(mismatch)는 `goal-driven-dev` 갭 리포트에 연결
- 디자인 토큰 갭은 `design-spec-build` Mode A로 전환

---

## 크롤러 없으면 설치

```bash
cd tools/visual-qa && npm install && npx playwright install chromium
```

## 기존 visual-qa와의 관계

| 스킬 | 목적 | 핵심 차이 |
|------|------|-----------|
| **visual-qa** | BFS 전체 사이트 크롤 + Spec 생성 | 사이트 전체를 탐색, Lean 4 Spec 출력 |
| **visual-qa-mosaic** | 단일/소수 페이지 타일 분석 | 한 페이지를 깊이 있게 분석, 레이아웃 비교 |
| **visual-qa-click** | 클릭 인터랙션 분석 | 버튼/링크 클릭 후 변화 추적 |

## 크롤러 위치

`tools/visual-qa/src/mosaic-crawl.ts`
