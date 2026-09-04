"""Export exact local model inputs. The output contains private local history."""
import argparse
from contextlib import closing
import json
import os
from pathlib import Path

from calibrate_bridge import connect, table_exists


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("database", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--since-ms", type=int, default=0)
    args = parser.parse_args()
    with closing(connect(args.database)) as connection:
        if not table_exists(connection, "bridge_forecast_replays"):
            parser.error("this database predates exact replay recording; run the new app build first")
        rows = connection.execute(
            "SELECT input_json,prediction_json FROM bridge_forecast_replays "
            "WHERE evaluated_at_ms>=? ORDER BY evaluated_at_ms", (args.since_ms,),
        )
        # Exclusive creation avoids accidentally replacing the source database
        # or another export; permissions keep vessel history local to its owner.
        descriptor = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        count = 0
        with os.fdopen(descriptor, "w") as output:
            for inputs, prediction in rows:
                output.write(json.dumps({"input": json.loads(inputs), "expected": json.loads(prediction)}) + "\n")
                count += 1
    print(f"Exported {count} evaluations to {args.output}")


if __name__ == "__main__":
    main()
