"""Run the existing RSSP test on immutable corpus captures, without editing RSSP."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def latest(root):
    rows = {}
    for line in (root / 'capture.jsonl').read_text(encoding='utf-8').splitlines():
        item = json.loads(line)
        rows[item['simfile']] = (root, item)
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('captures', type=Path)
    parser.add_argument('--overlay', type=Path)
    parser.add_argument('--regression-seeds', type=Path, help='explicitly recorded RSSP-only seeds for previously missing regression references')
    parser.add_argument('--test-bin', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    captures, output, test_bin = args.captures.resolve(), args.out.resolve(), args.test_bin.resolve()
    config = json.loads((captures / 'capture-config.json').read_text(encoding='utf-8'))
    if 'rssp_reference' not in config:
        parser.error('captures must include legacy-format RSSP baselines')
    packs = Path(config['packs'])
    if output == packs or packs in output.parents:
        parser.error('test output must be outside source packs')
    records = latest(captures)
    seeds = {}
    if args.regression_seeds:
        seed_manifest = json.loads((args.regression_seeds / 'seed-manifest.json').read_text(encoding='utf-8'))
        seeds = {item['md5']:item for item in seed_manifest['entries']}
    if args.overlay:
        overlay = latest(args.overlay.resolve())
        if not set(overlay) <= set(records):
            parser.error('overlay contains sources outside the captured corpus')
        records.update(overlay)
    output.mkdir(parents=True, exist_ok=True)
    view, baselines = output / 'packs', output / 'baseline'
    inputs = []
    journal = []
    for done, (name, (origin, record)) in enumerate(sorted(records.items()), 1):
        source = packs / name
        if digest(source) != record['compressed_source_sha256']:
            parser.error(f'source changed after capture: {name}')
        relative = name if name.endswith('.zst') else '_uncompressed/' + name + '.zst'
        target = view / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        if name.endswith('.zst'):
            if not target.exists():
                try:
                    os.link(source, target)
                except OSError:
                    shutil.copyfile(source, target)
            if digest(target) != record['compressed_source_sha256']:
                parser.error(f'test view changed: {relative}')
        else:
            with target.open('wb') as stream:
                subprocess.run(['zstd', '-q', '-3', '-c', str(source)], stdout=stream, check=True)
        md5 = record.get('source_md5')
        if md5:
            for suffix in ('.json.zst', '.rssp.json.zst'):
                candidate = origin / 'legacy' / md5[:2] / (md5 + suffix)
                if suffix == '.rssp.json.zst' and not candidate.exists() and md5 in seeds:
                    seed = seeds[md5]
                    candidate = args.regression_seeds / seed['file']
                    if seed['source_sha256'] != record['source_sha256'] or digest(candidate) != seed['regression_sha256']:
                        parser.error(f'regression seed digest changed: {name}')
                    record = dict(record, rssp_unique_reference_status='seeded', rssp_unique_seed_sha256=seed['regression_sha256'])
                dest = baselines / md5[:2] / candidate.name
                dest.parent.mkdir(parents=True, exist_ok=True)
                complete = suffix != '.json.zst' or record.get('rssp_baseline_status') == 'written'
                if complete and candidate.exists():
                    if not dest.exists() or digest(candidate) != digest(dest):
                        shutil.copyfile(candidate, dest)
                else:
                    dest.unlink(missing_ok=True)
        inputs.append(dict(test_input=relative, simfile=name, source_md5=md5,
                           capture_root=str(origin), source_sha256=record.get('source_sha256')))
        journal.append(dict(record, capture_root=str(origin)))
        if done % 5000 == 0 or done == len(records):
            print(f'{done}/{len(records)} verified test inputs prepared', flush=True)
    (output / 'test-inputs.json').write_text(json.dumps(inputs, ensure_ascii=False), encoding='utf-8')
    (output / 'capture.jsonl').write_text(''.join(json.dumps(row, ensure_ascii=False)+'\n' for row in journal), encoding='utf-8')
    config['capture_root'] = str(captures)
    config['overlay_root'] = str(args.overlay.resolve()) if args.overlay else None
    config['test_executable_sha256'] = digest(test_bin)
    if args.regression_seeds:
        config['regression_seeds'] = dict(path=str(args.regression_seeds.resolve()), manifest_sha256=digest(args.regression_seeds / 'seed-manifest.json'))
    (output / 'capture-config.json').write_text(json.dumps(config, indent=2), encoding='utf-8')
    env = dict(os.environ)
    env.pop('RSSP_NATIVE_BASELINE_DIR', None)
    env.pop('RSSP_PARITY_CASES_FILE', None)
    env.update(RSSP_PARITY_PACKS_DIR=str(view), RSSP_PARITY_BASELINE_DIR=str(baselines),
               RSSP_PARITY_RESULTS=str(output / 'parity-raw.jsonl'), RSSP_PARITY_QUIET='1')
    print(f'Running unchanged RSSP test against {len(inputs)} inputs', flush=True)
    with (output / 'parity.log').open('wb') as log:
        result = subprocess.run([str(test_bin)], env=env, stdout=log, stderr=subprocess.STDOUT)
    mapping = {row['test_input']:row['simfile'] for row in inputs}
    rows = [json.loads(line) for line in (output / 'parity-raw.jsonl').read_text(encoding='utf-8').splitlines()]
    if len(rows) != len(inputs):
        parser.error('test did not report every input')
    for row in rows:
        key = row['simfile'].replace('\\', '/')
        row['test_input'] = key
        row['simfile'] = mapping[key]
    if len({row['simfile'] for row in rows}) != len(inputs):
        parser.error('test reported duplicate inputs')
    (output / 'parity.jsonl').write_text(''.join(json.dumps(row, ensure_ascii=False)+'\n' for row in rows), encoding='utf-8')
    summary = dict(total=len(rows), passed=sum(r['passed'] for r in rows), failed=sum(not r['passed'] for r in rows),
                   exit_code=result.returncode, test_executable_sha256=digest(test_bin))
    (output / 'parity-run.json').write_text(json.dumps(summary, indent=2), encoding='utf-8')
    print(json.dumps(summary), flush=True)
    if result.returncode or summary['failed']:
        raise SystemExit(result.returncode or 1)


if __name__ == '__main__':
    main()
