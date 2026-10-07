"""Capture compressed, unchanged simfiles in isolated native harness processes."""
import argparse
import concurrent.futures
import hashlib
import json
import shutil
from pathlib import Path
import subprocess
import time


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('packs', type=Path)
    parser.add_argument('--harness', type=Path, required=True)
    parser.add_argument('--theme', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--jobs', type=int, default=4)
    parser.add_argument('--timeout', type=float, default=120)
    parser.add_argument('--limit', type=int)
    parser.add_argument('--cases-file', type=Path, help='JSON array of relative simfile paths')
    parser.add_argument('--rssp-reference', type=Path, help='preserve existing independent RSSP-only references alongside new legacy-format baselines')
    parser.add_argument('--theme-repairs', type=Path, help='pinned theme-only copies validated against original engine data')
    parser.add_argument('--resume', action='store_true')
    parser.add_argument('--upgrade-harness', action='store_true', help='record a new executable while preserving completed captures')
    args = parser.parse_args()
    if args.jobs < 1 or args.timeout <= 0:
        parser.error('jobs and timeout must be positive')
    packs, harness, theme, out = (p.resolve() for p in (args.packs, args.harness, args.theme, args.out))
    if out == packs or packs in out.parents:
        parser.error('output must be outside the source packs')
    files = sorted(p for p in packs.rglob('*') if p.is_file() and p.name.lower().endswith(('.sm.zst', '.ssc.zst', '.sm', '.ssc')))
    if args.cases_file:
        selected = set(json.loads(args.cases_file.read_text(encoding='utf-8')))
        files = [p for p in files if p.relative_to(packs).as_posix() in selected]
        if len(files) != len(selected):
            parser.error('cases-file contains missing or duplicate source paths')
    if args.limit is not None:
        files = files[:args.limit]
    config = dict(packs=str(packs), harness=str(harness), harness_sha256=digest(harness),
                  theme=str(theme), theme_scripts={name: digest(theme / 'Scripts' / name) for name in
                  ('SL-ChartParser.lua', 'SL-ChartParserHelpers.lua', 'SL-BPMDisplayHelpers.lua')},
                  loading_mode='direct_simfile_no_cache', repairs=[])
    if args.rssp_reference:
        config['rssp_reference'] = str(args.rssp_reference.resolve())
    if args.theme_repairs:
        config['theme_repairs'] = dict(path=str(args.theme_repairs.resolve()), sha256=digest(args.theme_repairs))
    out.mkdir(parents=True, exist_ok=True)
    config_path, journal_path = out / 'capture-config.json', out / 'capture.jsonl'
    previous = {}
    if config_path.exists():
        old_config = json.loads(config_path.read_text(encoding='utf-8'))
        if not args.resume:
            parser.error('output already exists; use resume')
        if old_config != config:
            unchanged = all(old_config.get(k) == v for k, v in config.items() if k != 'harness_sha256')
            if not args.upgrade_harness or not unchanged:
                parser.error('resume requires identical harness, sources, and theme; upgrade-harness only allows an executable change')
            with (out / 'capture-config-history.jsonl').open('a', encoding='utf-8') as history:
                history.write(json.dumps(old_config) + '\n')
            config_path.write_text(json.dumps(config, indent=2), encoding='utf-8')
        for line in journal_path.read_text(encoding='utf-8').splitlines():
            item = json.loads(line)
            previous[item['simfile']] = item
    else:
        config_path.write_text(json.dumps(config, indent=2), encoding='utf-8')

    def capture(path):
        relative = path.relative_to(packs).as_posix()
        target = out / 'native' / (relative + '.json.zst')
        source_digest = digest(path)
        old = previous.get(relative)
        if old and old['status'] == 'captured' and old['compressed_source_sha256'] == source_digest and target.exists():
            return old
        temporary = target.with_name(target.stem + '.pending.zst')
        result = dict(simfile=relative, compressed_source_sha256=source_digest, baseline=target.relative_to(out).as_posix(),
                      harness_sha256=config['harness_sha256'])
        started = time.monotonic()
        try:
            target.parent.mkdir(parents=True, exist_ok=True)
            command = [str(harness), 'charts', str(path), '--theme', str(theme), '--out', str(temporary)]
            if args.rssp_reference:
                command += ['--rssp-baseline', str(out / 'legacy')]
            if args.theme_repairs:
                command += ['--theme-repairs', str(args.theme_repairs)]
            proc = subprocess.run(command,
                                  capture_output=True, timeout=args.timeout)
            if proc.returncode:
                raise RuntimeError(f'exit {proc.returncode}: ' + proc.stderr.decode('utf-8', errors='replace')[-4000:])
            result.update(json.loads(proc.stdout))
            if args.rssp_reference:
                md5 = result['source_md5']
                result['rssp_baseline_status'] = 'unavailable' if result.get('rssp_error') else 'written'
                reference = args.rssp_reference / md5[:2] / (md5 + '.rssp.json.zst')
                result['rssp_unique_reference_status'] = 'available' if reference.exists() else 'unavailable'
                if reference.exists():
                    target_ref = out / 'legacy' / md5[:2] / reference.name
                    target_ref.parent.mkdir(parents=True, exist_ok=True)
                    # Copy unchanged independent expectations. Never generate
                    # RSSP-only values using the implementation under test.
                    if not target_ref.exists():
                        shutil.copyfile(reference, target_ref)
                    result['rssp_unique_reference_sha256'] = digest(reference)
            temporary.replace(target)
            result['status'] = 'captured'
        except (subprocess.TimeoutExpired, RuntimeError, ValueError, OSError) as error:
            result.update(status='unavailable', error=str(error))
            temporary.unlink(missing_ok=True)
        result['seconds'] = round(time.monotonic() - started, 3)
        return result

    counts = dict(total=len(files), captured=0, unavailable=0, charts=0, partial_theme_charts=0, unsupported_charts=0, raw_metadata_charts=0, diagnostic_files=0)
    if args.rssp_reference:
        counts.update(rssp_baselines=0, rssp_baselines_unavailable=0, rssp_unique_references_unavailable=0)
    started = time.monotonic()
    with journal_path.open('a', encoding='utf-8') as journal, concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for done, item in enumerate(pool.map(capture, files), 1):
            if item is not previous.get(item['simfile']):
                journal.write(json.dumps(item, ensure_ascii=False) + '\n')
                journal.flush()
            counts[item['status']] += 1
            counts['charts'] += item.get('charts', 0)
            counts['partial_theme_charts'] += len(item.get('partial_theme_charts', []))
            counts['unsupported_charts'] += len(item.get('unsupported_charts', []))
            counts['raw_metadata_charts'] += len(item.get('raw_metadata_charts', []))
            counts['diagnostic_files'] += bool(item.get('diagnostics'))
            if args.rssp_reference:
                counts['rssp_baselines'] += item.get('rssp_baseline_status') == 'written'
                counts['rssp_baselines_unavailable'] += item.get('rssp_baseline_status') != 'written'
                counts['rssp_unique_references_unavailable'] += item.get('rssp_unique_reference_status') != 'available'
            if done % 250 == 0 or done == len(files):
                print(f'{done}/{len(files)}: captured={counts["captured"]}, unavailable={counts["unavailable"]}, elapsed={time.monotonic()-started:.0f}s', flush=True)
    counts['seconds'] = round(time.monotonic() - started, 1)
    (out / 'capture-summary.json').write_text(json.dumps(counts, indent=2), encoding='utf-8')
    print(json.dumps(counts), flush=True)


if __name__ == '__main__':
    main()
