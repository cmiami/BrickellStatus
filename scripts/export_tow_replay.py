#!/usr/bin/env python3
"""Read-only, consistent export for the hosted tow replay. Keep output private."""
import argparse
import json
import sqlite3
import tempfile
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--db', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--tugs', nargs='+', default=['SARA', 'PEPIN'])
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='tow-replay-') as temporary:
        with sqlite3.connect(args.db.resolve().as_uri() + '?mode=ro', uri=True) as source:
            with sqlite3.connect(str(Path(temporary) / 'history.sqlite3')) as database:
                source.backup(database)
                database.row_factory = sqlite3.Row
                ledger = {row['mmsi']: dict(row) for row in database.execute('SELECT * FROM ais_vessel_ledger')}
                fixes = [dict(row) for row in database.execute('SELECT * FROM ais_track_fixes ORDER BY observed_at_ms')]
                transits = [dict(row) for row in database.execute('SELECT * FROM ais_transits ORDER BY crossed_at_ms')]
    names = {name.upper() for name in args.tugs}
    tug_ids = {key for key, row in ledger.items() if (row['name'] or '').upper() in names
               and row['vessel_class'] in ('tug', 'tug + tow')}
    episodes = []
    for transit in transits:
        if ledger.get(transit['mmsi'], {}).get('vessel_class') in ('tug', 'tug + tow'):
            continue
        matches = [t for t in transits if t['mmsi'] in tug_ids and t['direction'] == transit['direction']
                   and abs(t['crossed_at_ms'] - transit['crossed_at_ms']) <= 180_000]
        if matches:
            episodes.append({'cargo': transit['mmsi'], 'tugs': sorted({t['mmsi'] for t in matches}),
                             'at': transit['crossed_at_ms'], 'direction': transit['direction']})
    args.out.write_text(json.dumps({'ledger': ledger, 'fixes': fixes, 'episodes': episodes}))
    print(f'Exported {len(fixes)} fixes and {len(episodes)} candidate co-passages. These are not labelled tow outcomes.')


if __name__ == '__main__':
    main()
