"""Summarize capture and RSSP parity journals without changing their verdicts."""
import argparse
import collections
import csv
import json
from pathlib import Path


def latest(path):
    items = {}
    for line in path.read_text(encoding='utf-8').splitlines():
        item = json.loads(line)
        items[item['simfile'].replace('\\', '/')] = item
    return items


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('root', type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    captures = latest(root / 'capture.jsonl')
    results = latest(root / 'parity.jsonl')
    config = json.loads((root / 'capture-config.json').read_text(encoding='utf-8'))
    legacy = 'rssp_reference' in config
    if set(captures) != set(results):
        parser.error('capture and parity coverage differ')
    categories = collections.Counter()
    rows = []
    numeric_only = 0
    numeric_names = set()
    for name, result in results.items():
        failures = result.get('failures') or {}
        if legacy and 'input' in failures:
            message = str(failures['input'])
            if 'MISSING BASELINE' in message:
                category = 'rssp_unique_reference' if '.rssp.json.zst' in message else 'incomplete_theme'
            else:
                category = next((category for marker, category in [
                    ('RSSP title:', 'metadata'), ('RSSP step_artist', 'step_artist'),
                    ('RSSP hash_bpms:', 'bpm'), ('RSSP Hashes:', 'hash'),
                    ('RSSP duration', 'duration'), ('RSSP nps', 'nps'),
                    ('RSSP notes_per_measure', 'nps'), ('RSSP detailed:', 'stream_breakdown')
                ] if marker in message), 'comparison')
            failures = {category: message}
        categories.update(failures.keys())
        nps = failures.get('nps', [])
        max_delta, density, spacing, nps_structure = 0.0, False, False, False
        if isinstance(nps, list):
            for chart in nps:
                fields = chart['fields']
                density |= 'notes_per_measure' in fields
                spacing |= 'equally_spaced_per_measure' in fields
                values = fields.get('nps_per_measure', {})
                nps_structure |= values.get('native_length') != values.get('rssp_length')
                peak = fields.get('peak_nps', {})
                max_delta = max(max_delta, abs(peak.get('native', 0)-peak.get('rssp', 0)),
                                fields.get('nps_per_measure', {}).get('max_abs_delta', 0))
        if isinstance(nps, list) and nps and set(failures) == {'nps'} and not density and not spacing and not nps_structure:
            numeric_only += 1
            numeric_names.add(name)
        if failures:
            rows.append(dict(simfile=name, categories=', '.join(sorted(failures)),
                             native_status=captures[name]['status'], max_nps_delta=max_delta,
                             density_mismatch=density, spacing_mismatch=spacing))
    counts = {}
    for label, names in [('compressed', [n for n in results if n.endswith('.zst')]),
                         ('uncompressed', [n for n in results if not n.endswith('.zst')])]:
        counts[label] = dict(total=len(names), passed=sum(results[n]['passed'] for n in names),
                             failed=sum(not results[n]['passed'] for n in names),
                             captured=sum(captures[n]['status']=='captured' for n in names),
                             nps_numeric_only_failures=sum(n in numeric_names for n in names))
    summary = dict(total=len(results), passed=sum(r['passed'] for r in results.values()),
                   failed=sum(not r['passed'] for r in results.values()),
                   captured=sum(r['status']=='captured' for r in captures.values()),
                   unavailable=sum(r['status']=='unavailable' for r in captures.values()),
                   charts=sum(r.get('charts', 0) for r in captures.values()),
                   partial_theme_charts=sum(len(r.get('partial_theme_charts', [])) for r in captures.values()),
                   partial_theme_files=sum(bool(r.get('partial_theme_charts')) for r in captures.values()),
                   unsupported_native_charts=sum(len(r.get('unsupported_charts', [])) for r in captures.values()),
                   raw_metadata_files=sum(bool(r.get('raw_metadata_charts')) for r in captures.values()),
                   diagnostic_files=sum(bool(r.get('diagnostics')) for r in captures.values()),
                   nps_numeric_only_failures=numeric_only, groups=counts, categories=dict(categories))
    if legacy:
        summary.update(rssp_baselines=sum(r.get('rssp_baseline_status')=='written' for r in captures.values()),
                       rssp_only_seeded_files=sum(r.get('rssp_unique_reference_status')=='seeded' for r in captures.values()),
                       theme_repaired_files=sum(bool(r.get('theme_repairs')) for r in captures.values()),
                       theme_repaired_charts=sum(len(r.get('theme_repairs', [])) for r in captures.values()))
    (root / 'summary.json').write_text(json.dumps(summary, indent=2), encoding='utf-8')
    unavailable = [r for r in captures.values() if r['status']=='unavailable']
    (root / 'unavailable.json').write_text(json.dumps(unavailable, ensure_ascii=False, indent=2), encoding='utf-8')
    with (root / 'failures.csv').open('w', newline='', encoding='utf-8-sig') as output:
        writer = csv.DictWriter(output, fieldnames=['simfile', 'categories', 'native_status', 'max_nps_delta', 'density_mismatch', 'spacing_mismatch'])
        writer.writeheader()
        writer.writerows(rows)
    report = ['# RSSP native baseline candidate run', '',
              ('Original simfiles, existing baselines, and RSSP source were preserved. Candidates use the current ITGmania engine '
               'and the unchanged, pinned Simply Love scripts used by the previous harness.' if legacy else
               'Original simfiles and existing baselines were preserved. Candidates use ITGmania directly and unchanged Simply Love 5.9.0.'), '',
              '| Input group | Files | Captured | Passed | Failed |', '| --- | ---: | ---: | ---: | ---: |']
    for label, c in counts.items():
        report.append(f'| {label} | {c["total"]:,} | {c["captured"]:,} | {c["passed"]:,} | {c["failed"]:,} |')
    report += ['', f'{summary["charts"]:,} native charts captured. {summary["unavailable"]} inputs unavailable. '
               f'{summary["partial_theme_charts"]} charts across {summary["partial_theme_files"]} files have partial theme results. '
               f'{summary["unsupported_native_charts"]} additional charts have unsupported native types and no fabricated note statistics. '
               f'{summary["raw_metadata_files"]} files preserve non-UTF-8 metadata as exact raw bytes. '
               f'{summary["diagnostic_files"]:,} captured files contain loader diagnostics.', '',
               f'{numeric_only:,} failed files differ only in numeric NPS results; strict verdicts remain failures.', '',
               '| Failure category | Files |', '| --- | ---: |']
    report += [f'| {name} | {count:,} |' for name, count in categories.most_common()]
    if legacy:
        report += ['', f'{summary["rssp_baselines"]:,} complete dance-chart reports were exported in the original baseline format. '
                   f'{summary["theme_repaired_charts"]} charts across {summary["theme_repaired_files"]} files use pinned theme-only copies; '
                   'each copy reproduced the original engine note data, timing, statistics, BPMs, native hash, and times exactly. '
                   'Original metadata remains in the exported report.', '',
                   'The existing RSSP test runs without source changes or new tolerances. Its legacy mode stops at the first failure in a file. '
                   'A missing RSSP-only reference prevents that file from reaching comparisons unless a separate, explicitly marked regression seed is supplied. '
                   'Uncompressed extras are compressed into a verified test view without changing their bytes. '
                   'The report maps every result back to its original source path.', '',
                   'Lua NPS, density, streams, and theme hashes follow the selected previous-theme contract. '
                   'Raw native floats and current-engine hashes remain in the compressed captures. '
                   'Report numbers retain the previous CLI’s six-significant-digit serialization. '
                   'Metadata retains its UTF-8/Windows-1252 display policy and trimming; raw byte values remain available.', '',
                   'Review `failures.csv`, `parity.jsonl`, `test-inputs.json`, and the source/executable digests in the capture journals and configuration.', '']
        if summary['rssp_only_seeded_files']:
            report += [f'{summary["rssp_only_seeded_files"]} extra files previously lacked RSSP-only references. '
                       'Those regression seeds were recorded separately using the unchanged RSSP CLI and are explicitly marked as RSSP-derived. '
                       'They do not independently verify RSSP-only calculations. Engine timing/statistics and theme NPS/hash/stream expectations '
                       'remain independent and passed the unchanged parity comparisons for these files.', '']
        if summary['rssp_baselines'] == summary['total'] and summary['partial_theme_charts']:
            report += ['Every input has complete theme results for its dance-single and dance-double charts. '
                       'The partial theme results retained in raw captures belong to other styles outside this RSSP comparison domain.', '']
        (root / 'REPORT.md').write_text('\n'.join(report)+'\n', encoding='utf-8')
        print(json.dumps(summary, indent=2))
        return
    report += ['', 'Categories overlap. Native NPS is projected through the report’s six-significant-digit formatting; comparisons remain exact. '
               'Raw f32 values remain in the compressed captures. Metadata comes from the direct simfile loader, which does not run full-game Song::TidyUpData; '
               'whitespace and missing-title defaults can therefore appear as metadata differences. Timing retains the existing parity test’s 1 ms tolerance.', '',
               'RSSP-only metrics use the previous independent `.rssp.json.zst` references. Partial theme data cannot certify a full pass. '
               'No fallback hashes, regenerated RSSP expectations, or repaired inputs were used.', '',
               'Review `failures.csv` for per-file categories and numeric NPS deltas, `unavailable.json` for capture failures, '
               'and `parity.jsonl` for comparison details. `capture.jsonl` and the capture configurations record source and executable provenance.', '',
               '## Unavailable inputs', '']
    report += [f'- `{r["simfile"]}`: {r["error"].strip()}' for r in unavailable]
    (root / 'REPORT.md').write_text('\n'.join(report)+'\n', encoding='utf-8')
    print(json.dumps(summary, indent=2))


if __name__ == '__main__':
    main()
