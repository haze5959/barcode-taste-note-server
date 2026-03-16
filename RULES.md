# BarcodeTasteNote Server Context & Rules

## 1. Project Overview
시음노트 서비스(BarcodeTasteNote)의 백엔드 서버입니다.
Rust와 Actix-web을 기반으로 구축되었으며, PostgreSQL을 데이터베이스로 사용합니다.

## 2. Tech Stack & Libraries
| Component | Technology | Description |
|-----------|------------|-------------|
| **Language** | Rust 2024 | |
| **Framework** | Actix-web 4 | Web Framework |
| **Auth** | Auth0, JWT | `actix-web-httpauth`, `alcoholic_jwt` |
| **Database** | PostgreSQL | |
| **ORM** | Diesel 2.2 | `diesel`, `r2d2` |
| **Serialization**| Serde | JSON handling |
| **CDN** | Cloudflare R2 | Image Storage |
| **Logging** | env_logger | |

## 3. Project Structure
```
.
├── src/
│   ├── handlers/       # Request Handlers (Controllers)
│   │   ├── users_handler.rs
│   │   ├── products_handlers.rs
│   │   ├── notes_handlers.rs
│   │   └── ...
│   ├── utils/          # Utilities
│   │   ├── auth.rs     # Auth validators
│   │   └── ...
│   ├── models.rs       # Diesel Models & Structs
│   ├── schema.rs       # Diesel Schema (Auto-generated, **DO NOT EDIT**)
│   ├── errors.rs       # Custom Error Types
│   ├── main.rs         # App Entry point & Route definitions
│   └── auth.rs         # JWT Authentication Logic
├── tests/              # Integration tests
└── RULES.md            # This file
```

## 4. Key Patterns & Conventions

### 4.1. Authentication
- **Mechanism**: Bearer Token (JWT) via Auth0.
- **Header**: `Authorization: Bearer <token>`
- **Implementation**: `HttpAuthentication` middleware 사용.

### 4.2. API Response Format
모든 API 응답은 다음 공통 포맷을 엄격히 준수해야 합니다.

**Success:**
```json
{
    "result": true,
    "data": { ... } // Generic Data Model
}
```

**Failure:**
```json
{
    "result": false,
    "error": 100 // Error Code (See src/errors.rs)
}
```

### 4.3. Database (Diesel ORM)
- `src/schema.rs`는 Diesel CLI에 의해 자동 관리되므로 **수동 수정 금지**.
- DB 변경 시 마이그레이션 파일 생성 및 적용 필요.

## 5. Development Guidelines (Rules)

### 5.1. Coding Standards
- **Language**: 주석, 커밋 메시지, Implementation Plan 등 모든 설명 텍스트는 **한글(Korean)**로 작성.

### 5.2. Constraints
- **Dependencies**: 새로운 External Library 추가 **절대 금지** (유저 명시적 승인 시 예외).
- **File Restrictions**: `src/schema.rs` 수정 금지.

### 5.3. Reference
- Handler 예시: `src/handlers/users_handler.rs`
- Test 예시: `tests/user_test.rs`