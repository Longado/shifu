# Managed build caches

Developer builds reuse one cache per worktree/component. Defaults are an 8 GiB reclaimable-cache budget and 3 idle days. Active, current, unverified and source-containing directories remain protected; this is not a hard disk quota. Failed builds retain their own diagnostics while applying the same budget to older inactive caches. Source, installed applications, frozen candidates and installation backups are excluded.

```sh
npm --prefix app run build:desktop -- --debug
python3 scripts/build-cache.py run --kind cli -- cargo test --locked
python3 scripts/build-cache.py run --kind desktop -- cargo check --locked --manifest-path app/src-tauri/Cargo.toml
python3 scripts/build-cache.py plan
```

Only `prune --apply` explicitly applies the displayed cleanup plan; builds also reclaim eligible caches on exit. Legacy target directories are not automatically managed. Use the managed desktop entry rather than invoking `tauri build` directly. Test, candidate and backup app bundles belong beneath `.noindex` directories, so they do not add development copies to application search.

开发构建默认按8 GiB预算、闲置3天回收受管缓存；构建失败也执行相同预算，保留本次诊断。活跃或不能核实的目录保留，旧 target 不自动纳管。正式App、源码和冻结交付不在回收范围内。测试、候选及备份App统一放在 `.noindex` 父目录。
