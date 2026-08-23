---
name: migrate
description: "Use when migrating data from Excel/CSV/Markdown into SQLite — structured migration with backup, validation, and rollback"
---

# SHIFU: Migrate

> **Gears:** All gears. See `shifu:engage` for gear definitions.

## The Iron Law

```
BACKUP BEFORE YOU TOUCH, VALIDATE AFTER YOU WRITE
```

YOU MUST create a backup of the target database before any migration. YOU MUST validate row counts and sample data after insertion.

## The Process

### Step 1: Survey

Before writing any migration code:
1. **Read the source** — understand the structure (headers, row count, data types, empty rows)
2. **Read the target** — check if the table exists, what columns it has
3. **Map columns** — which source columns go to which target columns? Any transforms needed?
4. **Flag issues** — missing data, inconsistent formats, duplicates

Report findings to user before proceeding.

### Step 2: Backup

```bash
cp <target.db> <target.db>.pre-migration-$(date +%Y%m%d%H%M)
```

### Step 3: Create Table (if needed)

- Match column types to source data
- Add CHECK constraints for enum-like fields
- Add `created_at` and `updated_at` defaults
- Add foreign keys where relationships exist
- Add `updated_at` trigger

### Step 4: Migrate

Write a Python script using `openpyxl` (Excel) or `csv` (CSV) + `sqlite3`:
- Pad rows to expected column count (Excel sheets vary)
- Skip empty rows (check the primary data column, not just row[0])
- Strip whitespace from all string values
- Handle date formatting (`.strftime("%Y-%m-%d")` for datetime objects)
- Use `INSERT OR IGNORE` when deduplication is needed
- Print count after each batch

### Step 5: Validate

```sql
-- Row count matches expectation
SELECT count(*) FROM <table>;

-- Sample data looks correct
SELECT * FROM <table> LIMIT 5;

-- No obviously broken data
SELECT * FROM <table> WHERE <primary_column> IS NULL;
```

Report results with evidence. If counts don't match, investigate before proceeding.

### Step 6: Archive Source (optional)

If user confirms migration is correct:
```bash
mkdir -p .data/archived
mv <source_file> .data/archived/
```

## Common Pitfalls

| Pitfall | Prevention |
|---------|-----------|
| Excel sheets have different column counts | Pad rows with `while len(vals) < N: vals.append(None)` |
| Dates come as datetime objects | Check `hasattr(val, 'strftime')` before converting |
| Duplicate rows across sheets | Use `INSERT OR IGNORE` or check before insert |
| Empty rows in middle of data | Filter on the primary data column, not row index |
| Source file encoding issues | Use `openpyxl` for Excel (handles encoding), `csv` with `utf-8` for CSV |

## Red Flags — STOP

- About to run INSERT without a backup → STOP, backup first
- Row count after migration doesn't match source → investigate, don't proceed
- Modifying the source file instead of the target database → wrong direction
- Running migration twice without checking for duplicates → will double the data
