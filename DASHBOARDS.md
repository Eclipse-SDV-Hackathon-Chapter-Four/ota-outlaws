# Dashboard Design

How we design dashboards, and why. It builds on [DESIGN.md](DESIGN.md): same fonts, same colours, same calm. Read that first.

---

## The core idea

> **A dashboard is an answer, not a wall of data.**

Most dashboards show everything that can be measured and leave the user to work out what it means. We do the opposite. We start from the question a person opens the dashboard to answer, answer it in the first five seconds, and let them go deeper only if they want to.

The test is simple: someone opens the dashboard, glances for five seconds, and closes it. Do they know what they needed to know? If not, the dashboard has failed, however beautiful the charts are.

---

## Principles

1. **Questions before charts.** Every dashboard is built around 1–3 real questions from real users. A chart that answers none of them is removed.
2. **No naked numbers.** A number alone means nothing. Every value is shown against something: last period, a target, or what is normal.
3. **Calm by default, loud only when it matters.** When everything is fine, the dashboard is quiet. Attention is a budget; we spend it only on what needs action.
4. **Glance, scan, dig.** Three levels of depth, in that order. Never make someone dig to get the glance.
5. **Explain, don't just display.** When something changed, help the user see *why*, not only *that*.
6. **Every view leads somewhere.** Insight without a next step is trivia. Point to the action, the owner or the detail page.
7. **Honest data.** Show freshness, gaps and uncertainty. Never smooth, truncate or colour data to tell a nicer story.

---

## Signature concepts

These are the ideas that make our dashboards ours. They come from the user's needs, not from a chart library.

### 1. The headline sentence

Every dashboard opens with one written sentence that answers its main question, set in **Newsreader**:

> Sign-ups are *up 12%* this week, mostly from the new pricing page.

- Generated from the data, not written by hand, so it is always true.
- *Italic* marks the change, exactly like our headings ("Things I've *built.*").
- If nothing notable happened, it says so: *"A normal week. Nothing needs your attention."* That sentence is a feature, not a fallback.

### 2. Compared to what?

Every metric carries its reference, written out:

| Instead of | We show |
|---|---|
| `4,210` | `4,210` · +8% vs last week |
| `92%` | `92%` · target 95% |
| A line going up | A line inside a soft band showing the normal range |

The **normal band** is the quiet hero of our charts: a light `--accent-soft` area for the expected range. Inside the band, nothing to see. Outside, the point is marked. Users learn to read it in seconds.

### 3. Since you last looked

The dashboard remembers the user's last visit and quietly marks what changed since then: a small neutral dot next to the metric, and a line under the headline (*"3 things changed since Monday"*). No pop-ups, no badges with counts that demand to be cleared.

### 4. Why did it change?

Clicking a metric does not open a bigger version of the same chart. It opens its **drivers**: which segment, region, product or step caused the change, sorted by contribution. The question in the user's head is "why?", so that is what we answer.

### 5. Next step

Where a metric has an owner, a runbook, a related ticket or a detail page, the card links to it with our standard arrow button. The user should never have to leave and search for what to do next.

### 6. Shareable views

Filters, time range and selected metric live in the URL. A **"Copy link to this view"** button gives the same clear *Copied* feedback as the copy-email button on the portfolio. People discuss dashboards in chat; we make that easy and exact.

---

## Layout: glance, scan, dig

```
┌───────────────────────────────────────────────────────────┐
│  Title · time range · filters            Updated 2 min ago │  ← context
├───────────────────────────────────────────────────────────┤
│  The headline sentence, in Newsreader.                     │  ← GLANCE (5 s)
│  3 things changed since Monday.                            │
├──────────────┬──────────────┬──────────────┬──────────────┤
│  Key metric  │  Key metric  │  Key metric  │  Key metric  │  ← SCAN (1 min)
│  + reference │  + reference │  + reference │  + reference │
├──────────────┴──────────────┴──────────────┴──────────────┤
│  One main chart, with the normal band                      │
├───────────────────────────────────────────────────────────┤
│  Details: drivers, tables, breakdowns                      │  ← DIG (as long as needed)
└───────────────────────────────────────────────────────────┘
```

- **Maximum four key metrics** at the top. If everything is key, nothing is.
- **One main chart** per view. Small multiples are fine; a grid of eight unrelated charts is not.
- Reading order follows importance: top-left is the most important, always.
- Generous spacing, using the `--section` and `--pad-x` rhythm. Density is allowed in the *dig* layer, never in the *glance* layer.

---

## Typography

Still two fonts, nothing else.

| Role | Font | Used for |
|---|---|---|
| Voice | **Newsreader** | Dashboard title, the headline sentence, empty-state messages |
| Interface | **Geist** | Numbers, labels, axes, tables, filters, tooltips |

- **Numbers use tabular figures** (`font-variant-numeric: tabular-nums`) so they line up and don't jump when they update. This replaces any need for a monospace font.
- Key metric values are large and regular weight (400). Size gives importance, not boldness.
- Units are smaller and in `--text-2`: **4,210** <small>users</small>.
- Format for humans: `1.2M`, not `1,204,331`, at the glance level; the exact value is in the tooltip and the table.

---

## Colour

Our colour rules from DESIGN.md hold. Dashboards add a few specific ones.

- **Green means "look here", not "good".** It marks the selected series, the active filter, the highlighted point. It does not mean a number is healthy.
- **Up is not automatically good.** Rising costs, error rates or churn are bad news. So changes are shown with an arrow and words (`↑ 8%`), in neutral text, and the meaning comes from the sentence and the reference, not from red/green.
- **One extra token, used rarely: `--attention`.** For the few things that truly need action (a broken target, a failing system). Defined in `src/style.css` for both themes, always paired with an icon and words so it never relies on colour alone. If more than one or two things on screen use it, the dashboard is too loud.
- **Charts are mostly neutral.** Context series use `--text-3` / `--border-2`; the series that matters uses `--accent`. One highlighted series per chart.
- Multi-series charts that truly need categories use tints of the accent and neutrals, in a fixed order, documented once. Never a rainbow.
- Gridlines are barely there (`--border`). The data is the brightest thing on the chart.

---

## Charts

Choose the chart by the question, not by variety.

| The question | The chart |
|---|---|
| How is it moving over time? | Line, with the normal band |
| How do these compare? | Horizontal bars, sorted |
| What is it made of? | Stacked bar or a simple ranked list. No pies with more than 3 slices. |
| Where is the drop-off? | Funnel as horizontal bars, with step-to-step % |
| Is this one number OK? | Big number + reference + small sparkline |

Rules:

- **Label directly.** Put the series name at the end of its line instead of in a legend.
- **Axes start at zero for bars.** Lines may zoom in, but say so.
- **Tooltips show the comparison**, not only the value: *"Tue 4,210 · +6% vs last Tue"*.
- **Tables are first-class.** Right-aligned numbers, sortable columns, the sort direction visible. Many users trust a table more than a chart; give them both.
- Every chart has a short text alternative describing the main point, for screen readers and for the "copy" action.

---

## States

A dashboard spends much of its life not in the happy state. These are designed, not forgotten.

| State | What the user sees |
|---|---|
| **Loading** | Calm skeletons in the real layout shape. No spinners in every card. The headline appears last, once it can be true. |
| **Empty** | A Newsreader sentence explaining why (*"No orders yet in this period"*) and what would fill it. |
| **Partial / delayed data** | The affected metric is marked *"Data until 14:00"*, and the latest stretch of the line is dashed. |
| **Stale** | The "Updated" time turns into a clear note: *"Last updated 3 hours ago. Refresh"*. |
| **Error** | Plain language, what failed, what still works, and a retry. Never a stack trace, never a blank card. |
| **Nothing notable** | Said proudly in the headline. A calm dashboard is a good dashboard. |

---

## Motion

Same rules as DESIGN.md: small, short, purposeful, off under `prefers-reduced-motion`.

- Numbers update with a short cross-fade, **not** a rolling counter.
- Lines draw in once on first load (≤ 0.6s), never again on filter changes. On filter changes, values morph in place so the eye can follow what moved.
- Hover feedback on chart points and table rows: a colour change and a thin crosshair, nothing more.
- No auto-rotating carousels, no live-ticking numbers unless the user is watching something live and asked for it.

---

## Interaction details

These small things are the design. Keep them working.

- **Time range** is always visible and always tells you what you are comparing with (*"Last 7 days vs previous 7 days"*).
- **Filters** show as removable chips; a single *"Reset"* returns to the default view.
- **Keyboard:** every card, chart point and filter is reachable; arrow keys move through chart points; focus is always visible.
- **Freshness:** a quiet *"Updated 2 min ago"* in the header, the same spirit as the live Aachen time.
- **Touch:** anything shown on hover (tooltips, drivers) is reachable with a tap.
- **Theme:** both dark and light mode are designed, checked and readable. Charts use tokens, so they switch with the page.
- **Export:** the current view as CSV and as a link. Exports respect the active filters.

---

## Mobile

A phone dashboard is not a shrunken desktop dashboard. It is the **glance** layer done perfectly.

- Headline sentence, then key metrics stacked one per row, then the main chart.
- The *dig* layer becomes tappable drill-downs, not tiny tables.
- No horizontal scrolling for the page. Wide tables scroll inside their own container, with the first column pinned.

---

## Voice

- Write like a helpful colleague, not like a database: *"Sign-ups dropped on Sunday"*, not *"Δ sign_up_count: −14.2%"*.
- Name metrics the way users talk about them. Technical names go in the tooltip.
- Never overstate. *"Mostly from the pricing page"* only if the data supports *mostly*.

---

## Code structure

The same calm, separated structure as the portfolio.

- **Data, logic and view are separate.** Metric definitions (name, unit, format, reference, good direction, owner link) live in one config file. Components only render.
- **Headline sentences come from templates** in the config, filled from data, so the copy is reviewable in one place.
- **Small reusable pieces:** `MetricCard`, `HeadlineSentence`, `TrendLine` (with normal band), `RankedBars`, `DataTable`, `StateMessage`, `TimeRangePicker`, `FilterChips`.
- **Colours only from tokens.** Chart libraries read from CSS variables; nothing hard-coded.
- **Numbers are formatted in one helper** (`formatMetric`) so `1.2M`, `%` and dates look the same everywhere.

---

## What we don't do

- Speedometer gauges, 3D charts, donuts used as decoration.
- Red/green as the only signal of good or bad.
- Twelve KPI tiles of equal size competing for attention.
- Animated counters, auto-refreshing flicker, celebratory confetti.
- Charts added because the space was empty.

---

## Checklist for any new dashboard

- [ ] Which real user question does it answer, and is the answer visible in five seconds?
- [ ] Is there a headline sentence, including a *"nothing notable"* version?
- [ ] Does every number have a reference (last period, target, or normal range)?
- [ ] Is green used for focus, not for "good"? Is `--attention` used only where action is needed?
- [ ] Can the user see *why* something changed, and where to go next?
- [ ] Are loading, empty, partial, stale and error states designed?
- [ ] Is data freshness shown honestly?
- [ ] Only Newsreader / Geist, tabular numbers, existing tokens?
- [ ] Works in **both** dark and light mode, on mobile, with a keyboard and on touch?
- [ ] Is motion small, purposeful and off under reduced motion?
- [ ] Are metric definitions and copy in config, not in components?
