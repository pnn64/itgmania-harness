"""Record missing RSSP-only regression seeds, separately from independent parity data."""
import argparse
import concurrent.futures
import hashlib
import json
from pathlib import Path
import subprocess


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('results', type=Path)
    parser.add_argument('--rssp-bin', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--jobs', type=int, default=4)
    args = parser.parse_args()
    root, binary, output = args.results.resolve(), args.rssp_bin.resolve(), args.out.resolve()
    config = json.loads((root / 'capture-config.json').read_text(encoding='utf-8'))
    packs = Path(config['packs'])
    if output == packs or packs in output.parents:
        parser.error('regression seed output must be outside source packs')
    rows = {json.loads(line)['simfile']:json.loads(line) for line in (root / 'capture.jsonl').read_text(encoding='utf-8').splitlines()}
    selected = [r for r in rows.values() if r.get('rssp_unique_reference_status') == 'unavailable']
    output.mkdir(parents=True, exist_ok=True)
    binary_sha = digest(binary)

    def seed(record):
        source = packs / record['simfile']
        if digest(source) != record['compressed_source_sha256']:
            raise ValueError(f'source changed: {source}')
        raw = subprocess.check_output(['zstd', '-dc', str(source)]) if source.suffix == '.zst' else source.read_bytes()
        md5 = hashlib.md5(raw).hexdigest()
        if md5 != record['source_md5']:
            raise ValueError(f'source MD5 changed: {source}')
        ext = source.with_suffix('').suffix if source.suffix == '.zst' else source.suffix
        staged = output / 'inputs' / (md5 + ext)
        staged.parent.mkdir(parents=True, exist_ok=True)
        staged.write_bytes(raw)
        result = subprocess.run([str(binary), str(staged), '--json'], capture_output=True, check=True)
        actual = json.loads(result.stdout)
        # Keep only the fields consumed by RSSP-only regression comparisons.
        # Engine/theme expectations are never copied from this output.
        charts = []
        for chart in actual['charts']:
            value = dict(chart_info={k:chart['chart_info'][k] for k in ('step_type', 'difficulty', 'rating', 'matrix_rating')},
                         breakdown=chart['breakdown'], stream_info={'sn_breaks':chart['stream_info']['sn_breaks']})
            value.update({k:chart[k] for k in ('mono_candle_stats', 'pattern_counts') if k in chart})
            charts.append(value)
        regression = dict(charts=charts,
                          regression_seed=dict(provider='unchanged RSSP CLI; not an independent parity oracle',
                                               executable_sha256=binary_sha, source_sha256=hashlib.sha256(raw).hexdigest()))
        target = output / md5[:2] / (md5 + '.rssp.json.zst')
        target.parent.mkdir(parents=True, exist_ok=True)
        data = json.dumps(regression, ensure_ascii=False).encode('utf-8')
        compressed = subprocess.run(['zstd', '-q', '-3', '-c'], input=data, capture_output=True, check=True).stdout
        target.write_bytes(compressed)
        return dict(simfile=record['simfile'], md5=md5, source_sha256=hashlib.sha256(raw).hexdigest(),
                    regression_sha256=digest(target), file=target.relative_to(output).as_posix())

    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        entries = list(pool.map(seed, selected))
    manifest = dict(provider='RSSP-only regression seeds; independent engine/theme baselines are separate',
                    rssp_executable_sha256=binary_sha, arguments=['INPUT', '--json'], entries=entries)
    (output / 'seed-manifest.json').write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding='utf-8')
    print(f'Recorded {len(entries)} missing RSSP-only regression seeds; no engine/theme expected values generated')


if __name__ == '__main__':
    main()
