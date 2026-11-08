"""Infer RRULEs for recurring series from crawled occurrences.

Input: JSONL from `tec-crawl --format full` (legacy API). Series are grouped by the
occurrence URL pattern /event/{slug}/{YYYY-MM-DD}/.
"""
import collections
import datetime as dt
import json
import re
import sys

from dateutil.rrule import DAILY, MONTHLY, WEEKLY, YEARLY, rrule, weekday

DAYS = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"]
PAT = re.compile(r"/event/([^/]+)/(\d{4}-\d{2}-\d{2})/?$")


def load(path):
    series = collections.defaultdict(list)
    for line in open(path):
        e = json.loads(line)
        m = PAT.search(e.get("url") or "")
        if m:
            series[m.group(1)].append(e)
    return series


def candidates(dates):
    first = dates[0]
    wds = sorted({d.weekday() for d in dates})
    # DAILY with a constant gap
    for k in range(1, 8):
        yield (f"FREQ=DAILY;INTERVAL={k}" if k > 1 else "FREQ=DAILY",
               dict(freq=DAILY, interval=k))
    # WEEKLY on the observed weekday set, intervals 1-4
    for k in range(1, 5):
        rule = "FREQ=WEEKLY" + (f";INTERVAL={k}" if k > 1 else "") + ";BYDAY=" + ",".join(DAYS[w] for w in wds)
        yield rule, dict(freq=WEEKLY, interval=k, byweekday=wds)
    # MONTHLY by day of month
    for k in (1, 2, 3):
        yield (f"FREQ=MONTHLY" + (f";INTERVAL={k}" if k > 1 else "") + f";BYMONTHDAY={first.day}",
               dict(freq=MONTHLY, interval=k, bymonthday=first.day))
    # MONTHLY by nth / last weekday (single weekday series)
    if len(wds) == 1:
        w = wds[0]
        nths = {(d.day - 1) // 7 + 1 for d in dates}
        lasts = {(d + dt.timedelta(days=7)).month != d.month for d in dates}
        for k in (1, 2, 3):
            iv = f";INTERVAL={k}" if k > 1 else ""
            if len(nths) == 1:
                n = nths.pop(); nths.add(n)
                yield (f"FREQ=MONTHLY{iv};BYDAY={n}{DAYS[w]}",
                       dict(freq=MONTHLY, interval=k, byweekday=weekday(w)(n)))
            if lasts == {True}:
                yield (f"FREQ=MONTHLY{iv};BYDAY=-1{DAYS[w]}",
                       dict(freq=MONTHLY, interval=k, byweekday=weekday(w)(-1)))
    yield "FREQ=YEARLY", dict(freq=YEARLY)


def fit(dates):
    """Return the best (rule, exdates, rdates) or None."""
    first, last = dates[0], dates[-1]
    have = set(dates)
    best = None
    for rule, kw in candidates(dates):
        gen = {d.date() for d in rrule(dtstart=dt.datetime.combine(first, dt.time()), until=dt.datetime.combine(last, dt.time()), **kw)}
        missing = sorted(gen - have)   # rule produces, site lacks  -> EXDATE
        extra = sorted(have - gen)     # site has, rule lacks       -> RDATE
        # Prefer: fewest corrections, then simplest rule (first yielded).
        score = (len(missing) + 2 * len(extra), len(rule))
        if best is None or score < best[0]:
            best = (score, rule, missing, extra)
    _, rule, missing, extra = best
    # Reject fits where corrections outweigh the rule itself.
    if len(missing) + len(extra) > max(1, len(dates) // 3):
        return None
    return rule, missing, extra


def main(path):
    series = load(path)
    results = {"exact": [], "with_exceptions": [], "no_fit": [], "too_few": []}
    for slug, occ in sorted(series.items(), key=lambda kv: -len(kv[1])):
        occ.sort(key=lambda e: e["start_date"])
        dates = [dt.date.fromisoformat(e["start_date"][:10]) for e in occ]
        times = {e["start_date"][11:16] for e in occ}
        durs = {e["end_date"] and (dt.datetime.fromisoformat(e["end_date"]) - dt.datetime.fromisoformat(e["start_date"])) for e in occ}
        title = occ[0]["title"]
        info = dict(slug=slug, title=title, n=len(dates), first=str(dates[0]), last=str(dates[-1]),
                    times=sorted(times), durations=sorted(str(d) for d in durs), tz=occ[0]["timezone"],
                    dates=[str(d) for d in dates])
        if len(dates) < 3:
            results["too_few"].append(info)
            continue
        f = fit(dates)
        if f is None:
            results["no_fit"].append(info)
            continue
        rule, missing, extra = f
        info.update(rule=rule, exdates=[str(d) for d in missing], rdates=[str(d) for d in extra])
        results["exact" if not missing and not extra else "with_exceptions"].append(info)
    json.dump(results, sys.stdout, indent=1)


if __name__ == "__main__":
    main(sys.argv[1])
