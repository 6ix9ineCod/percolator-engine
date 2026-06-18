# I built the five missing pieces of a perp risk engine — then tried to break it

Here's a thing most people never see: a perpetual-futures exchange is basically a
**risk engine** wearing a trading screen. Behind every "long" and "short" is a system
deciding, every fraction of a second, who's still solvent, who gets liquidated, and
who eats the loss when someone can't pay. Get that engine wrong and the whole venue
blows up. Most of the famous crypto exchange disasters are really *risk-engine*
disasters.

[Percolator](https://github.com/aeyakovenko/percolator), by Anatoly Yakovenko, is a
beautiful little version of that engine. Instead of the usual messy "auto-deleverage
and pray," it uses a simple rule: prices and funding can only move so far per step,
and losses get shared out in a clean, predictable way. Elegant.

But it has a catch — and the catch is the interesting part.

## The dangerous part is the part everyone skips

Percolator is honest about what it *doesn't* do. It hands the hardest jobs to
"whoever integrates this": where do prices come from, and how do you stop someone
faking them? How do you charge for risk? What happens when the underlying market is
**closed** but the perp keeps trading 24/7? How do you know your insurance fund is
actually big enough?

That "we'll leave that to the integrator" list? **That's exactly where real exchanges
get hacked.** Oracle manipulation, funding games, overnight gaps — they all live in
the gap Percolator leaves open.

So I built the integrator. All of it. Five small, focused pieces — and then I built a
sixth thing whose only job is to **attack** the other five and try to break them.

## The five pieces (in plain English)

**The pricing engine** charges traders a tiny fee that reflects how risky their
position actually is — more leverage, more crowding on the losing side, more
volatility, more fee. Not "insurance" in the promise-to-pay sense; an honest,
risk-tuned toll. *(100 tests.)*

**The calibrator** answers the scary question: "how bad could one really bad day
get?" It uses the same math insurers and weather modelers use for once-in-a-century
events (extreme-value statistics + correlated-shock simulation) to size the fee so
the fund survives the 1-in-200 day. And it does this by calling the *real* pricing
engine, so the math can never quietly drift apart. *(33 tests.)*

**The price feed** is the anti-manipulation layer — the big one. The obvious design
is "detect cheating and halt trading." I threw that out: a halt is a panic button a
griefer can mash, and halting mid-stress is often what *causes* the death spiral.
Instead it never accuses anyone — it just *measures confidence* (do the exchanges
agree? is there real depth? is the data fresh?) and quietly slows down **withdrawals**
on a shaky price while never blocking liquidations. Two prices: a fast one for
liquidations, a patient confirmed one for cashing out. Net effect: cheating the price
stops being profitable, and nobody ever has to flip a kill-switch. Runs in ~45
billionths of a second per update. *(25 tests.)*

**The equity engine** handles the weird problem of a stock perpetual: the perp trades
24/7, but the stock it tracks goes home at 4pm. My first design trusted a calendar to
know when markets are open — and I killed it, because one wrong holiday in a config
file mis-prices the entire book. The rebuilt version trusts the *market itself*: if
the price feed goes quiet when the calendar says "open," it treats that as a halt, no
schedule required. It also refuses to list anything it can't safely price overnight —
because the safest risk machinery is the kind you don't have to build. ~15 billionths
of a second per update. *(40 tests.)*

**The engine** snaps all four together behind a single function call and runs the
whole loop end to end. This is the keystone — the thing that turns four clever islands
into one working system.

## And then the fun part: I tried to break it

The sixth piece is a **red-team** — an automated attacker that hammers the *real*
assembled system with thousands of random adversarial sequences (fake price moves,
overnight closes, leverage spikes, liquidations) and checks, after every single move,
whether any safety rule got violated. When it finds a break, it automatically
*shrinks* the attack down to the shortest version that still breaks it.

Why bother? Because **bugs hide in the seams** — in how the pieces connect, not in any
one piece. And the seams are exactly where my tests found real problems:

- **A phantom-payout bug** — the accounting recorded the fee it *meant* to charge
  instead of the fee it *actually* collected, quietly inflating what the fund thought
  it had. Found, reproduced, fixed.
- **An overflow that only appears when you connect two pieces** — the "obvious" way
  to wire confidence into the engine crashed it. Invisible in either piece alone;
  caught the moment they touched.
- **Insurance that wasn't actually running** — the first assembly charged premiums
  *on paper* but never moved the money into the fund. The whole insurance layer was a
  no-op until I asked the dumb question: "wait, does the money actually move?" It
  didn't. Now it does.
- **A clean result, too** — once everything was wired, the attacker *couldn't* drain
  the fund below what it owed — the bounded-price design means liquidations fire while
  traders are still solvent. That's not me failing to find a bug; that's evidence the
  core idea holds up.

And the attacker isn't for show: it has a built-in **honesty check** — it's run
against a deliberately broken version first and *must* catch that planted bug, so its
"all clear" on the real system actually means something.

## The honest part

This is serious engineering, not a live exchange. Every risk estimate comes from price
history, not from real liquidation records; some of the modeling is an educated
assumption; it's a fork-plus-additions, not an audited deployment. I'm telling you
that up front on purpose — **a risk system that brags about guarantees it can't keep
is the first one to blow up.** The whole point here is the opposite: measure honestly,
say what you don't know, and let an automated adversary keep you honest.

## How I built it (the playbook)

Same move every time: start from a *proven* idea, add one *new* twist — and say out
loud why the obvious design is wrong before building the better one. Keep the
fast-path code lean and deterministic (no floating point, no memory allocation, fast
enough for high-frequency trading), with all the heavy lifting in a separate test
harness. Write the test first, every time. And always, always write down what you're
*not* sure about.

## See it

- **The pricing engine** → https://github.com/6ix9ineCod/percolator-insurer
- **The calibrator** → https://github.com/6ix9ineCod/percolator-calibration
- **The price feed** → https://github.com/6ix9ineCod/percolator-feed
- **The equity engine** → https://github.com/6ix9ineCod/percolator-equity
- **The engine + red-team** → https://github.com/6ix9ineCod/percolator-engine
- Built on [`aeyakovenko/percolator`](https://github.com/aeyakovenko/percolator) (Apache-2.0)
