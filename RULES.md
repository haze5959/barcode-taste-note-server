# BarNote Server Context & Rules

## 1. Project Overview
This is the backend server for **BarNote** (a tasting note service).
It is built with **Rust** and **Actix-web**, utilizing **PostgreSQL** as the primary database.

## 2. Tech Stack & Libraries
| Component | Technology | Description |
|-----------|------------|-------------|
| **Language** | Rust 2024 | Core programming language |
| **Framework** | Actix-web 4 | High-performance web framework (`actix-web`, `actix-rt`) |
| **Auth** | Auth0, JWT | Handled via `actix-web-httpauth`, `alcoholic_jwt` |
| **Database** | PostgreSQL | Relational DB, also uses `pgvector` for embeddings |
| **ORM** | Diesel 2.2 | `diesel`, `r2d2` for connection pooling |
| **Serialization**| Serde | JSON handling (`serde`, `serde_json`, `serde_derive`) |
| **Storage/CDN**| Cloudflare R2 | Image storage via `aws-sdk-s3` |
| **Logging** | env_logger | Application-level logging (`env_logger`, `log`) |
| **AI Integration**| OpenAI & Gemini | Vector embeddings & AI features via `async-openai` and custom utils |
| **Workspaces**| Multiple | `crawler`, `batch` exist as cargo workspace members |

## 3. Project Structure
```text
.
├── Cargo.toml          # Workspace & Dependencies config
├── src/
│   ├── main.rs         # App Entry point, Server config & Route definitions
│   ├── lib.rs          # Exposes modules for integration tests or workspace members
│   ├── handlers/       # Request Handlers (Controllers) (e.g., users, products, notes, admin)
│   ├── utils/          # Utilities (auth, db, r2, gemini, openai, fcm, scraper, etc.)
│   ├── models.rs       # Diesel Models, DB Structs, Request/Response Structs
│   ├── schema.rs       # Diesel Schema (Auto-generated, **DO NOT EDIT**)
│   ├── errors.rs       # Custom Error Enum (`CommonResponseError`) & HTTP mappers
│   ├── auth.rs         # JWT Authentication & Validation Logic
│   └── constants.rs    # Application wide constants
├── migrations/         # Diesel migration files (SQL up/down)
├── crawler/            # Workspace member for web crawling scripts
├── batch/              # Workspace member for batch jobs
└── RULES.md            # This file
```

## 4. Key Patterns & Conventions

### 4.1. Authentication
- **Mechanism**: Bearer Token (JWT) provided via Auth0.
- **Header**: `Authorization: Bearer <token>`
- **Implementation**: Utilizes `HttpAuthentication::bearer(auth::validator)` middleware in Actix-web to protect private routes.

### 4.2. API Response Format
All API responses **must** strictly adhere to the following common format (using `CommonResponse<T>` from `src/models.rs`):

**Success:**
```json
{
    "result": true,
    "data": { ... } // Generic Data Model (or null/None)
}
```

**Failure:**
```json
{
    "result": false,
    "error": 100 // Error Code Enum (See src/errors.rs)
}
```

### 4.3. Error Handling
- Use the custom `CommonResponseError` enum defined in `src/errors.rs`.
- Diesel database errors should be cleanly mapped to `CommonResponseError` (e.g., `NotFound` to `RecordNotFound`, etc).
- Handlers should return `Result<HttpResponse, CommonResponseError>`.
- Log the errors accurately inside mapper functions before responding to the user.

### 4.4. Database (Diesel ORM)
- The file `src/schema.rs` is **automatically generated and managed** by the Diesel CLI. **NEVER manually edit this file.**
- If a database schema change is needed:
  1. Create a new migration file using diesel CLI.
  2. Apply the migration using Diesel CLI.
  3. Update `src/models.rs` to reflect the new table or columns.
- The project uses `pgvector` for AI similarity search. Models map the vector embedding to `Option<Vector>`.

## 5. Development Guidelines (Rules)

### 5.1. AI Agent Communication & Commit Standards
- **Language Policy**: While this `RULES.md` is written in English to save tokens during AI sessions, **all source code comments, git commit messages, implementation plans, and descriptions MUST be written in Korean (한국어).**
- Always thoroughly read the project's logic and understand the flow before suggesting changes. 
- Explain your reasoning clearly before making significant architectural changes.

### 5.2. Constraints & Security
- **Dependencies**: Adding new external libraries in `Cargo.toml` is **strictly forbidden** unless explicitly approved by the user.
- **File Restrictions**: As previously stated, `src/schema.rs` is read-only.
- **State Management**: App State (like DB pools, R2 clients) is passed to routes via Actix `web::Data`. Extract it in handler functions cleanly.

### 5.3. Typical Development Workflow
1. **Route Definition**: Add the new route mapping in `src/main.rs`. Ensure you put it in the correct scope (public vs authenticated API scope vs admin).
2. **Models**: Add required Request/Response and Database structs in `src/models.rs`.
3. **Handler**: Implement the actual logic in `src/handlers/<feature>_handlers.rs`.
4. **Errors**: If a new specific error is needed, add it to `src/errors.rs`.

### 5.4. References
- **Handler Example**: Look at `src/handlers/users_handler.rs` or `src/handlers/notes_handlers.rs`.
- **Error Example**: Check `src/errors.rs` for `ResponseError` implementation.