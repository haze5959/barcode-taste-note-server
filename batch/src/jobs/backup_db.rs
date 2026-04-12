use std::env;
use std::path::PathBuf;
use std::process::Command;
use chrono::Local;

/// rclone remote 이름 (환경변수 RCLONE_REMOTE로 덮어쓸 수 있음, 기본값: "r2")
const DEFAULT_RCLONE_REMOTE: &str = "r2";

/// rclone 업로드 대상 버킷/경로 (환경변수 RCLONE_BACKUP_PATH로 덮어쓸 수 있음, 기본값: "barnote-backup/db")
const DEFAULT_RCLONE_BACKUP_PATH: &str = "barnote-backup/db";

/// pg_dump 덤프 파일을 저장할 로컬 임시 디렉토리 (기본값: "/tmp/barnote_backup")
const DEFAULT_DUMP_DIR: &str = "/tmp/barnote_backup";

pub async fn run() {
    println!("=== DB Backup Job 시작 ===");

    // 1. 환경변수에서 DATABASE_URL 파싱
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let db_config = parse_database_url(&database_url);
    println!("대상 DB: {}@{}/{}", db_config.user, db_config.host, db_config.dbname);

    // 2. 덤프 파일명 결정 (타임스탬프 포함)
    let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let dump_filename = format!("barnote_db_{}.sql", timestamp);
    let dump_dir = env::var("DUMP_DIR").unwrap_or_else(|_| DEFAULT_DUMP_DIR.to_string());
    let dump_path = PathBuf::from(&dump_dir).join(&dump_filename);

    // 3. 덤프 디렉토리 생성
    std::fs::create_dir_all(&dump_dir).expect("덤프 디렉토리 생성 실패");
    println!("덤프 경로: {}", dump_path.display());

    // 4. pg_dump 실행
    run_pg_dump(&db_config, &dump_path);

    // 5. rclone으로 R2에 업로드
    let rclone_remote = env::var("RCLONE_REMOTE").unwrap_or_else(|_| DEFAULT_RCLONE_REMOTE.to_string());
    let rclone_path = env::var("RCLONE_BACKUP_PATH").unwrap_or_else(|_| DEFAULT_RCLONE_BACKUP_PATH.to_string());
    let rclone_dest = format!("{}:{}", rclone_remote, rclone_path);

    run_rclone_copy(&dump_path, &rclone_dest);
    
    // 6. 오래된 백업 파일 삭제 (3일 기준)
    run_rclone_cleanup(&rclone_dest);

    // 7. 로컬 임시 덤프 파일 삭제
    match std::fs::remove_file(&dump_path) {
        Ok(_) => println!("로컬 덤프 파일 삭제 완료: {}", dump_path.display()),
        Err(e) => eprintln!("로컬 덤프 파일 삭제 실패 (무시): {}", e),
    }

    println!("=== DB Backup Job 완료 ===");
    println!("업로드 위치: {}/{}", rclone_dest, dump_filename);
}

// ─── pg_dump ───────────────────────────────────────────────────────────────

fn run_pg_dump(db: &DbConfig, dump_path: &PathBuf) {
    println!("pg_dump 실행 중...");

    let mut cmd = Command::new("pg_dump");
    cmd.arg("--no-password")
        .arg("--format=plain")
        .arg("--file")
        .arg(dump_path)
        .arg("--host").arg(&db.host)
        .arg("--port").arg(&db.port)
        .arg("--username").arg(&db.user)
        .arg(&db.dbname);

    // 비밀번호 환경변수로 전달 (pg_dump는 PGPASSWORD를 읽음)
    if let Some(ref pw) = db.password {
        cmd.env("PGPASSWORD", pw);
    }

    let status = cmd
        .status()
        .expect("pg_dump 실행 실패 — pg_dump가 설치되어 있는지 확인하세요");

    if !status.success() {
        panic!("pg_dump 실패 (exit code: {:?})", status.code());
    }

    println!("pg_dump 완료: {}", dump_path.display());
}

// ─── rclone ────────────────────────────────────────────────────────────────

fn run_rclone_copy(dump_path: &PathBuf, rclone_dest: &str) {
    println!("rclone 업로드 중: {} → {}", dump_path.display(), rclone_dest);

    // rclone copy <파일> <remote:bucket/path>
    let status = Command::new("rclone")
        .arg("copy")
        .arg(dump_path)
        .arg(rclone_dest)
        .arg("--quiet")
        .status()
        .expect("rclone 실행 실패 — rclone이 설치되어 있는지 확인하세요");

    if !status.success() {
        panic!("rclone copy 실패 (exit code: {:?})", status.code());
    }

    println!("rclone 업로드 완료");
}

fn run_rclone_cleanup(rclone_dest: &str) {
    println!("3일 이상 경과한 오래된 백업 파일 정리 중: {}", rclone_dest);

    // rclone delete <remote:path> --min-age 7d
    let status = Command::new("rclone")
        .arg("delete")
        .arg(rclone_dest)
        .arg("--min-age")
        .arg("3d")
        .status()
        .expect("rclone cleanup 실행 실패 — rclone이 설치되어 있는지 확인하세요");

    if !status.success() {
        eprintln!("rclone cleanup 실패 (exit code: {:?})", status.code());
    } else {
        println!("rclone cleanup 완료");
    }
}

// ─── DATABASE_URL 파서 ──────────────────────────────────────────────────────

struct DbConfig {
    user: String,
    password: Option<String>,
    host: String,
    port: String,
    dbname: String,
}

/// `postgres://user:password@host:port/dbname?...` 형태를 파싱합니다.
fn parse_database_url(url: &str) -> DbConfig {
    // URL 파싱 (간단하게 직접 파싱)
    // 형식: postgres://user:pass@host:port/dbname?opts
    let stripped = url
        .trim_start_matches("postgres://")
        .trim_start_matches("postgresql://");

    // user:pass@rest 분리
    let (userinfo, hostinfo) = stripped
        .split_once('@')
        .expect("DATABASE_URL 형식 오류: '@' 없음");

    // user와 password 분리
    let (user, password) = if userinfo.contains(':') {
        let (u, p) = userinfo.split_once(':').unwrap();
        (u.to_string(), Some(p.to_string()))
    } else {
        (userinfo.to_string(), None)
    };

    // host:port/dbname?opts 분리
    let hostinfo_no_opts = hostinfo.split('?').next().unwrap_or(hostinfo);
    let (hostport, dbname) = hostinfo_no_opts
        .split_once('/')
        .expect("DATABASE_URL 형식 오류: '/' (dbname) 없음");

    // host와 port 분리
    let (host, port) = if hostport.contains(':') {
        let (h, p) = hostport.split_once(':').unwrap();
        (h.to_string(), p.to_string())
    } else {
        (hostport.to_string(), "5432".to_string())
    };

    DbConfig {
        user,
        password,
        host,
        port,
        dbname: dbname.to_string(),
    }
}
