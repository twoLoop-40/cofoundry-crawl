---
name: visual-qa
description: "MCP 크롤러(cofoundry-crawl) + Playwright 크롤러를 하이브리드로 사용하여 사이트를 분석하고 Lean 4 Spec을 생성하는 워크플로우. `/visual-qa generate <url>` (크롤링), `/visual-qa run` (분석+Spec 생성) 두 단계."
user-invocable: true
---

# Visual QA — MCP + Playwright 하이브리드 크롤 → AI 분석 → Lean 4 Spec

> **⚠️ DAG-First 규칙 필수**: `~/.claude/skills/lean-dag-rules/SKILL.md` 참조.
> Spec 생성 시 import ≥ 1, canonical 타입 사용, sorry 금지.

## 아키텍처

```
Agent (이 스킬 = 운영 규칙 + 조건 판단)
  │
  ├── cofoundry-crawl MCP ──── 핵심 코드 (Rust)
  │   ├── login              API 로그인 → 세션 토큰
  │   ├── crawl_url          단일 URL 크롤 + 렌더링
  │   ├── screenshot         전체 페이지 스크린샷
  │   ├── render_batch       병렬 멀티페이지 크롤
  │   ├── search_site        BFS + 키워드 검색
  │   └── extract_content    구조화 추출
  │
  └── Playwright (보조) ──── tools/visual-qa/src/
      ├── credential-prompt  headed 브라우저로 로그인 UI
      ├── spa-crawl          React SPA fill 기반 로그인
      ├── click-crawl        인터랙션 분석
      └── mosaic-crawl       타일 분할 촬영
```

**원칙**: 핵심 코드는 MCP에, 운영 규칙과 조건 판단은 이 스킬에.

## 크롤러 자동 선택 규칙

| 조건 | 크롤러 | 이유 |
|------|--------|------|
| 공개 URL (외부, .onion) | **MCP** `crawl_url render=true` | 빠름 (~1.7s) |
| 인증 필요 + API 로그인 가능 | **MCP** `login` → `crawl_url cookies=...` | API 직접 호출 |
| 인증 필요 + SPA 폼 로그인만 | **Playwright** `spa-crawl.ts` | fill() 기반 |
| 인터랙션 분석 (클릭, hover) | **Playwright** `click-crawl.ts` | DOM 이벤트 필요 |
| 타일 레이아웃 분석 | **Playwright** `mosaic-crawl.ts` | viewport 분할 |
| MCP 실패 시 | **Playwright로 폴백** | - |

### 인증 판단 흐름

```
URL 수신
  │
  ├── localhost / 127.0.0.1 / 내부 IP / --auth* 있음
  │   └── 인증 필요 → Credential Resolution
  │
  └── 외부 URL
      ├── MCP crawl_url → 200 + 콘텐츠 있음 → 완료
      └── MCP crawl_url → login 리다이렉트 / 빈 페이지
          └── 인증 필요 → Credential Resolution
```

## Credential Resolution (범용 Human-in-the-Loop 패턴)

```
Agent 자율 시도 → 실패 → Human 개입 → 성공 검증 → 학습(저장) → 다음부터 자율
```

인증이 필요하면 아래 순서로 크리덴셜을 찾는다.
모든 로직은 `tools/visual-qa/src/credential-prompt.ts`에 구현.
**표준 키**: `CRAWL_AUTH_EMAIL` / `CRAWL_AUTH_PASSWORD` (credential-prompt가 정의)

### Step 1: CLI 인자 확인

`--auth-email=...`, `--auth-password=...`가 있으면 즉시 사용.

### Step 2: `.env` 표준 키 우선 탐색

`CRAWL_AUTH_EMAIL`/`CRAWL_AUTH_PASSWORD`가 `.env`에 있으면 즉시 사용.
(이전 실행에서 로그인 성공 후 자동 저장된 값)

### Step 3: `.env` 범용 패턴 스캔

표준 키가 없으면 기존 키를 패턴 매칭으로 탐지.

```
탐지 패턴 (대소문자 무시):
  EMAIL: *EMAIL*, *USERNAME*, *USER*, *LOGIN*, ^id$, *_id$ (UUID/API_ID 제외)
  PASSWORD: *PASSWORD*, *PASSWD*, ^pw$, *_pw$ (JWT_SECRET/API_SECRET 제외)
```

**페어링**: 같은 접두사를 공유하는 키를 한 쌍으로 묶음.
- `QA_TEST_EMAIL` + `QA_TEST_PASSWORD` → 한 쌍
- `ADMIN_USER` + `ADMIN_PW` → 한 쌍
- 단독 `id`/`pw` → 범용 쌍

### Step 4: E2E 테스트 설정 파일 스캔

`.env`에서 못 찾으면:
```
**/e2e/config.* | **/test/config.* | cypress.env.json | .playwright/auth.*
```

### Step 5: Playwright UI 프롬프트 (Human-in-the-Loop)

크리덴셜을 못 찾으면 **headed 브라우저 창**을 열어 사용자에게 직접 묻는다.

```
Playwright headed mode → 로그인 폼 UI 표시
  ├── 사용자가 email/password 입력 + "Continue Crawling" 클릭
  ├── "Skip" 클릭 → 인증 없이 크롤 진행
  └── 창 닫기 → 인증 없이 크롤 진행
  타임아웃: 5분
```

### Step 6: 로그인 성공 후 자동 저장

UI 프롬프트에서 입력받은 크리덴셜로 **실제 로그인 성공** 시에만 `.env`에 저장.

```
로그인 시도 → 성공 (login 페이지에서 벗어남)
  → .env에 CRAWL_AUTH_EMAIL / CRAWL_AUTH_PASSWORD 자동 기록
  → 다음 실행 시 Step 2에서 즉시 발견 → UI 프롬프트 없이 자동 로그인
로그인 시도 → 실패
  → 저장하지 않음 → 다음에도 UI 프롬프트 재표시
```

**보안**: 비밀번호는 로그/출력에 평문 노출 금지. 마스킹(`***`) 표시.

## 2단계 워크플로우

### Phase 1: `/visual-qa generate <url>` — 크롤링

#### Step 1: 크롤러 선택 + Credential Resolution

위 자동 선택 규칙에 따라 크롤러를 결정하고, 필요 시 크리덴셜을 확보한다.

#### Step 2: 크롤 실행

각 페이지에서 추출:
- **스크린샷** (full-page PNG)
- **DOM 구조** (heading, 폼, 테이블, 버튼, 링크)
- **API 호출** (network intercept — method, path, request/response body)
- **인증 감지** (로그인 폼, protected page redirect)

**MCP 사용 시**:
```bash
# Agent가 MCP 도구를 직접 호출
login(url, email, password)          → cookies
crawl_url(url, render=true, cookies) → content + links
screenshot(url, cookies)             → PNG base64
render_batch(urls, cookies)          → 병렬 크롤
```

**Playwright 사용 시**:
```bash
cd tools/visual-qa
# 크리덴셜 자동 탐지 (--auth-* 없으면 .env 스캔 → UI 프롬프트)
npx tsx src/spa-crawl.ts <url>
npx tsx src/spa-crawl.ts <url> --auth-email=... --auth-password=...
```

**결과**: `output/crawl-result.json` + `output/screenshots/*.png`

#### Playwright 옵션

| 옵션 | 기본값 | 설명 |
|------|--------|------|
| `--depth=N` | 3 | 최대 크롤링 깊이 (index.ts BFS) |
| `--max-pages=N` | 100 | 최대 페이지 수 (index.ts BFS) |
| `--output=DIR` | ./output | 결과 디렉토리 |
| `--timeout=MS` | 30000 | 페이지당 타임아웃 |
| `--auth-email=STR` | (auto) | 로그인 이메일/유저명 |
| `--auth-password=STR` | (auto) | 로그인 비밀번호 |
| `--login-url=URL` | `<origin>/login` | 로그인 페이지 URL |
| `--no-prompt` | false | Playwright UI 프롬프트 비활성화 |
| `--routes=FILE` | (auto-discover) | JSON 라우트 목록 (spa-crawl 전용) |
| `--detail-pattern=STR` | - | 디테일 페이지 패턴 (e.g., `/items/{id}`) |
| `--api-pattern=STR` | `/api/,/auth/` | API 인터셉트 URL 패턴 (콤마 구분) |

**SPA 라우트 발견**: `spa-crawl.ts`는 `--routes` 미지정 시 로그인 후 nav/sidebar 링크를 자동 스캔하여 라우트를 발견합니다. 특정 라우트만 크롤하려면 JSON 파일로 지정:
```json
["/", "/dashboard", "/settings", "/users"]
```

### Phase 2: `/visual-qa run` — AI 분석 + Spec 생성

크롤링 데이터를 AI 에이전트가 분석하여 Lean 4 Spec을 생성.

#### Step 1: 데이터 로드

`crawl-result.json` 읽기. summary 확인:
- 전체 페이지 수, API 엔드포인트, 폼, 테이블, 인증 여부

#### Step 2: 목적별 에이전트 분석 (병렬)

| Agent | 입력 | 추출 |
|-------|------|------|
| **UI Agent** | screenshots + headings + links | Route 구조, 네비게이션 트리, 컴포넌트 목록 |
| **API Agent** | apiEndpoints + response bodies | Endpoint 스키마, 필드 타입, 상태 코드 |
| **Logic Agent** | forms + tables + siteMap | 상태 전이, 폼 유효성, 비즈니스 규칙 |

각 에이전트는 `crawl-result.json`의 관련 섹션과 스크린샷을 참고.

#### Step 3: Lean 4 DAG 타입시스템 생성

> **공통 규칙**: `~/.claude/skills/visual-qa-types/SKILL.md` 참조.
> L1 공통 타입은 `Specs/UI/VisualQATypes.lean`, 페이지별 L2/L3는 이 스킬이 생성.

##### visual-qa가 생성하는 Lean 파일

| Layer | 파일 | 내용 |
|-------|------|------|
| L2 | `Specs/UI/<App>RouteSpec.lean` | 라우트 열거 + 보호 수준 매핑 (total function) |
| L2 | `Specs/UI/<App>APISpec.lean` | API 엔드포인트 목록 + 필수 필드 + 인증 요구 |
| L2 | `Specs/UI/<App>FormSpec.lean` | 폼 필드 + 유효성 + 의존타입 제약 |
| L3 | `Specs/UI/<App>FlowSpec.lean` | 상태 전이 + 인증 플로우 검증 |

##### 매핑 테이블

| 크롤링 데이터 | Lean 4 패턴 | Layer |
|-------------|-------------|-------|
| URL 목록 + 네비게이션 | `inductive Route` + `def routeProtection` | L2 RouteSpec |
| API endpoint + response | `EndpointMeta` (L1 공통) + `def routeEndpoints` | L2 APISpec |
| 폼 필드 + 유효성 | `structure FormField` + 의존타입 제약 | L2 FormSpec |
| 상태 전이 (페이지 흐름) | `StateTransition` (L1 공통) + `def validTransition` | L3 FlowSpec |
| 인증 플로우 | `RouteProtection` (L1 공통) + `theorem allRoutesProtected` | L2 RouteSpec |

#### Step 4: 사용자 대화

생성된 타입시스템 요약을 제시:
```
## 사이트 분석 결과
- 12 라우트 (7 protected) → RouteSpec.lean
- 8 API 엔드포인트 → APISpec.lean
- 3 폼 (login, register, search) → FormSpec.lean
- DAG: VisualQATypes → RouteSpec/APISpec/FormSpec → FlowSpec

### 다음 단계
1. 어떤 기능부터 구현할까요?
2. 추가할 비즈니스 규칙이 있나요?
3. 기존 사이트와 비교할까요?
```

#### Step 5: 이후 워크플로우

`spec-qa-verify` → 구현 → QA 사이클로 이어감.
`visual-qa-mosaic`로 레이아웃 확인, `visual-qa-click`으로 인터랙션 검증 가능.

## 사전 조건: 크롤러 자동 생성

스킬 첫 실행 시 아래 순서대로 확인하고, 없는 것만 설치한다.

### Step 0: MCP 크롤러 확인 + 안내

cofoundry-crawl MCP가 등록되어 있는지 확인한다.

```
확인: ~/.claude/.mcp.json에 "cofoundry-crawl" 키가 있는가?
  ├── 있음 → Step 1로 진행
  └── 없음 → 사용자에게 안내:
        "cofoundry-crawl MCP 크롤러가 설치되지 않았습니다.
         MCP 없이도 Playwright 크롤러로 동작하지만,
         MCP를 설치하면 공개 URL 크롤이 ~1.7초로 빨라집니다.

         설치 (선택):
         curl -fsSL https://raw.githubusercontent.com/twoLoop-40/cofoundry-crawl/main/install.sh | bash

         설치 후 Claude Code를 재시작하세요."
      → 설치 여부와 관계없이 Step 1로 진행 (Playwright만으로도 동작)
```

### Step 1: Playwright 크롤러 소스 복사

`tools/visual-qa/` 디렉토리가 없으면 아래 순서로 생성한다.

#### Step 1a: 소스 복사

스킬 디렉토리에 소스가 포함되어 있다. Read → Write로 복사:

```
소스 위치: ~/.claude/skills/visual-qa/sources/
  ├── index.ts            — BFS 크롤러 (visual-qa)
  ├── spa-crawl.ts        — SPA 인증 크롤러
  ├── click-crawl.ts      — 클릭 인터랙션 (visual-qa-click)
  ├── mosaic-crawl.ts     — 타일 분할 촬영 (visual-qa-mosaic)
  ├── credential-prompt.ts — 범용 크리덴셜 탐지 + UI
  └── package.json        — dependencies
```

```bash
mkdir -p tools/visual-qa/src
```

그 후 Read 도구로 `~/.claude/skills/visual-qa/sources/` 각 파일을 읽고,
Write 도구로 `tools/visual-qa/src/` 및 `tools/visual-qa/package.json`에 작성.

#### Step 1b: 의존성 설치

```bash
cd tools/visual-qa && npm install && npx playwright install chromium
```

#### Step 1c: .gitignore 추가

```bash
echo "tools/visual-qa/node_modules/" >> .gitignore
echo "tools/visual-qa/output*/" >> .gitignore
```

> **visual-qa-click**, **visual-qa-mosaic** 스킬도 같은 `tools/visual-qa/` 디렉토리를 사용.
> 이 스킬(visual-qa)의 사전 조건이 실행되면 3개 스킬 모두 사용 가능.
