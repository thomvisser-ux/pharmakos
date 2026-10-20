<!--
SPDX-FileCopyrightText: 2026 Pharmakos contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Radar lab: research memo

**Status: the radar lab's plan of record (decisions-log §2.7 item 140 (1)).** Written on
2026-10-14 for item 138 (4) (f), and revised the same day after its two reviews (contracts and
architecture; numbers and sources). It changes no code, no proto and no rules row. Its engineering
calls (section 10) are decided, open to the owner's overrule; its remaining operator questions
(section 10) stay open. Where it and a §2.7 item disagree, the item wins.

## 0. Why this memo exists

Item 138 (4) left radar on two paths and asked for a memo before the owner chose. On 2026-10-14
the owner chose: *"start by building radar realism to match published research. later we will try
making improvements and see how the modifications work in the sim."* That is item 138's path 3: a
**radar lab**, a walled, float-allowed crate built first as a test-bed whose models reproduce
published results, and only later a source of modifications that are ported into the deterministic
sim.

The owner then added a later roadmap item, also recorded in decisions-log item 140 (2): *"future
future roadmap addition of secondary illuminators of opportunity - receivable by the same passive
receiver that is used for catching solar ioo. different antennae configurations but same relative
location, height being variable by use of tower or tethered drone"*; the owner has since replaced the tethered drone with a tethered balloon (decisions-log item 147). It is roadmap, not committed
scope (phase 6, section 9), but it binds the architecture now: the passive receiver is one site
with a variable height, several antenna configurations and a set of illuminators (section 8.6), so
the solar phase is built in a shape that takes a second illuminator without a rewrite.

"Match published research" sets the bar for the lab: every model ships with **golden cases taken
from a published source**, each with its inputs, the published output, the source's tolerance and
the page, table or figure it comes from; or, where a case is an exact or formula value that no
source prints, with the formula and the two independent implementations that agree on it (flag X,
below). Section 7 collects them; the rest of the memo explains the models they pin.

Conventions. SI units throughout unless a source works in other units, in which case the case
quotes the source's units. `dB` is `10 log10` of a power ratio; `dBsm` is decibels relative to
1 m². `k` is Boltzmann's constant, 1.380649 × 10⁻²³ J/K (exact since the 2019 SI redefinition),
and `T₀` is the IEEE standard reference temperature, 290 K. A figure marked **computed here** is
this memo's own arithmetic from published inputs, and the reference script in appendix A
reproduces every one (section 7.10). Where it reproduces a published output, it makes that output
V-calc (below). Where it is an exact or formula value that no source prints, it is X (below) and
may be a golden, because a second, independent implementation agrees on it. Where it is an
estimate that rests on assumptions, as in section 5.5, it is never a golden.

## 1. Summary

- **The textbook models are well pinned.** The radar range equation, Swerling detection, Albersheim's
  and Shnidman's equations, the Marcum Q function, the horizon, constant-gamma clutter and Doppler
  have fully specified worked examples or exact closed forms in open sources (section 7). MTI's
  improvement factors and the Swerling definitions were only recalled in this research and wait in
  section 7.9. Phases 1 to 3 can be built test-first.
- **Passive radar with an illuminator of opportunity is well published but less fully specified.**
  The performance-prediction literature gives tabulated illuminator figures and worked detection
  ranges; real system papers give integration times, cancellation depths and detection ranges, but
  rarely every input. Its goldens are a mix of fully specified link budgets and partially specified
  measured results, flagged as such.
- **Solar passive radar is real, but it sounds ice sheets, not vehicles.** The published work uses
  the Sun's broadband radio emission as a noise illuminator to find strong specular echoes from
  large surfaces (an ice-sheet bed, a sea surface) by autocorrelation. Nothing published detects a
  vehicle with it. An idealised link budget (section 5.5, computed here) says a 10 m² target is not
  out of reach at short range with a dedicated reference antenna, but the effects such a budget
  leaves out (ground clutter at the same delay, the Sun's variability and bursts, coherent
  integration limits on a moving target, the extended Sun's spatial coherence) are exactly the ones
  a lab exists to study.
- **Four published values need care.** MathWorks' passive bistatic example drops its own 5 dB
  noise figure, so its `K` is 5 dB high (section 4.6); the published Swerling 0 figure of
  13.1217 dB is Shnidman's approximation, not the exact Marcum value of 13.1835 dB, so each
  function is tested against its own formula (section 2.3); the same page's Swerling 1 figures
  (13.5033 dB, and a fluctuation loss of 8.1696 dB) come from Barton's universal equation, not
  from Shnidman's, so they wait in section 7.9 until the lab has that function (section 2.3); and
  no published work detects a vehicle-sized target with the Sun as illuminator (section 5.1).
- **f64 is enough everywhere**, provided tail probabilities are computed directly and never as
  `1 − p` (section 8.2).
- **Phases 1 to 6 need no new dependency.** A small in-crate FFT, special functions, a complex
  least-squares solver and a seeded generator are reasonable and testable; `rustfft` is the one
  dependency worth asking for later, and only on a measured need (section 8.4).
- **The lab's numbers are guarded like any untrusted input** (section 8.7): the wall lifts the cast
  lints, so the lab carries its own rules for non-finite values, float-to-integer conversion and
  allocation sizes.

### How to read the flags

Every published figure in this memo carries the flag the research gave it:

| Flag | Meaning | May become a golden test? |
|---|---|---|
| **V** | Read on the source page itself. | Yes. |
| **V-calc** | Reproduced independently from the stated inputs, matching a published output (by the research or by this memo's own arithmetic, which is said where it applies). | Yes. |
| **X** | An exact or formula value that no source prints for these inputs, confirmed by two independent implementations: the reference script of appendix A (section 7.10) and the independent recomputation in this memo's numbers review of 2026-10-14. The exact detection values are also computed by two methods inside the script. | Yes. |
| **S** | Seen only in a search-result summary, not on the source page. | No: it goes in the "to verify" list (section 7.9). |
| **R** | Recalled, not verified. | No: it goes in the "to verify" list. |

Formulae that are textbook-standard but whose page was not read in this research are flagged R as
well; the lab may still implement them, but its golden tests pin them only through a V, V-calc or
X case.

## 2. Monostatic active radar

### 2.1 The radar range equation

The lab uses the **energy form**, which is what a matched filter delivers and what the MathWorks
reference pages implement (V):

`SNR = P_t τ G_t G_r λ² σ / ((4π)³ k T_s R⁴ L)`, with `T_s = T · F_n`,

where `P_t` is peak power, `τ` the pulse length (or, after pulse compression, the transmitted
energy divided by `P_t`), `G_t` and `G_r` the antenna gains, `λ` the wavelength, `σ` the radar
cross-section, `R` the range, `L` the product of all losses and `T_s` the system noise temperature.
The power form replaces `τ` by `1/B_n`, the noise bandwidth. With `n` pulses integrated, `SNR`
gains the integration gain of section 2.4.

**System noise temperature.** The textbook decomposition is Blake's: the antenna temperature, plus
the receive line's loss, plus the receiver's effective input temperature `T_e = T₀ (F_n − 1)`
referred to the antenna (Blake 1986; Blake, NRL Report 6930, 1969). The MathWorks pages use the
simpler `T_s = T · F_n` and a `systemp` helper; case R-5 pins the latter (`systemp(4.1)` gives
745.41 K, V), and Blake's full decomposition waits for a V case (section 7.9: the NRL 6930
worksheet could not be read because DTIC was unavailable during the research).

**Losses.** Transmit and receive line losses, atmospheric loss, beam-shape loss, scan loss, CFAR
loss, MTI loss, integration loss and fluctuation loss all multiply into `L`. The MathWorks
detectability-factor page tabulates several of them for one radar (V), but the inputs behind each
were not all recorded, so they sit in section 7.9 rather than among the goldens.

Sources: Skolnik 2001, ch. 2; Richards 2014, ch. 2; Richards, Scheer and Holm 2010, ch. 2;
Barton 2013; Blake 1986; MathWorks `radareqsnr` and `radareqrng` reference pages.

### 2.2 Radar cross-section and the Swerling models

The lab treats `σ` as an input in m² (dBsm in tables). Two things are modelled:

- **Canonical shapes** for sanity tests: the optical-region sphere (`σ = π a²`) and the flat plate
  at normal incidence (`σ = 4π A²/λ²`) are textbook results (R: Skolnik ch. 2; Knott, Shaeffer and
  Tuley); they are self-consistency tests, not published goldens.
- **Fluctuation.** Swerling's four cases (Swerling 1960) and the steady target (Swerling 0, also
  called Swerling 5 or Marcum's case): cases 1 and 2 have a chi-square RCS with two degrees of
  freedom (an exponential power distribution: many comparable scatterers), cases 3 and 4 four
  degrees of freedom (one dominant scatterer plus many small ones); cases 1 and 3 fluctuate scan to
  scan, 2 and 4 pulse to pulse. These definitions are standard but flagged R here (section 7.9); the
  lab's goldens pin them through D-1 (Swerling 1's closed form) and D-6 and D-7 (Swerling 2 and 1
  at `n = 10`), which only come out right for the right distribution.

Published RCS values for vehicles were not verified in this research. The lab should take `σ` as a
scenario input and not ship a table of "typical" values until one is read on a source page.

### 2.3 Detection probability against false-alarm probability

The receiver compares a detected statistic with a threshold set by `P_fa`. For a steady target
and one pulse, `P_d = Q₁(√(2·SNR), √(−2 ln P_fa))`, the Marcum Q function (Marcum 1960). For
`n` pulses noncoherently integrated by a square-law detector, the generalised `Q_M`; for the
fluctuating cases, Swerling's results (Swerling 1960). The lab implements three families and tests
**each against its own formula**:

1. **Exact.** Marcum Q and the incomplete gamma function, by a published stable algorithm: Shnidman
   1989 (the probability of detection and the generalised Marcum Q), Helstrom 1992 (saddle-point
   inversion), or Gil, Segura and Temme 2014 (relative accuracy about 1e-12 for parameters up to
   200, V). For Swerling 1 with one pulse there is a closed form, `P_d = P_fa^(1/(1+SNR))`, which
   gives 21.1436 dB at `P_d = 0.9`, `P_fa = 1e-6` (V; V-calc 21.1436 dB).
2. **Albersheim's equation** (Albersheim 1981), for a linear detector and a steady target:
   `SNR_dB = −5 log10 n + (6.2 + 4.54/√(n + 0.44)) · log10(A + 0.12 A B + 1.7 B)`, with
   `A = ln(0.62/P_fa)` and `B = ln(P_d/(1 − P_d))` (V). Its accuracy of about 0.2 dB over its stated
   range is R.
3. **Shnidman's equation** (Shnidman 2002), which covers Swerling 0 to 4 and `n` pulses. Its
   accuracy of about ±0.5 dB is R.

A fourth family appears on the MathWorks detectability-factor page: for Swerling 1 to 4, when `P_d`
lies in [0.2, 1 − 1e-6] and `P_fa < 1e-4`, the page's `detectability` uses **Barton's universal
equation** (Barton 2013), and Shnidman's equation for Swerling 0 only (V, read on the page by this
memo's numbers review). The lab has no Barton function until that equation is read on the page, so
the page's Swerling 1 values wait in section 7.9.

**Issue: the published Swerling 0 figure is Shnidman's, not Marcum's.** The MathWorks
detectability-factor page gives `D₀ = 13.1217 dB` for one pulse at `P_d = 0.9`, `P_fa = 1e-6`
(V). That is Shnidman's approximation. The exact square-law Marcum value is **13.1835 dB**
(X; the reference script's series and its quadrature both give 13.1835 dB), and Albersheim gives
**13.1145 dB** (X). The three disagree by up to 0.07 dB, inside the
accuracy usually quoted for the two approximations (R). So the lab's `shnidman()` is tested against 13.1217, its
`albersheim()` against 13.1145, and its `marcum_q()` against 13.1835, and no test checks one
function against another's published number. The same holds at `n = 10`: the detectability page's
Swerling 1 value of 13.5033 dB is Barton's universal equation, not Shnidman's; Shnidman's own value
is 13.5805 dB (X) and the exact value 13.4996 dB (X). And Albersheim's 4.9904 dB stands against the
exact square-law Swerling 0 value of 5.2675 dB (X). Albersheim fits a linear detector and the exact value here is
for a square-law detector, so part of that 0.28 dB gap may be the detector law rather than the
approximation; the lab can settle it by computing both laws.

**Coherent detection** of a known **complex** signal in white Gaussian noise has
`P_d = ½ erfc(erfcinv(2 P_fa) − √SNR)` (V, MathWorks `rocsnr`, its `NonfluctuatingCoherent` case);
at 8 dB and `P_fa = 0.01` that is 0.8899 (X). A real signal is a different case: its threshold is
`20 log10 Q⁻¹(P_fa)` (V-calc against `npwgnthresh`), and its `P_d = Q(Q⁻¹(P_fa) − √SNR)` would be
0.5736 at the same inputs (computed here). The lab names which signal each function is for.

### 2.4 Coherent and noncoherent integration

- **Coherent** integration of `n` pulses (phase kept, a slow-time FFT or sum) gains a factor `n` in
  SNR, less any straddle and mismatch loss. This is standard (R: Richards 2014, ch. 6) and is pinned
  in the lab through the range-Doppler map of phase 3.
- **Noncoherent** integration (magnitudes summed after detection) gains less than `n`. The
  integration gain is the drop in required single-pulse SNR: Shnidman's `G_i` for Swerling 0 from
  `n = 1` to `n = 10` is 7.7881 dB (V), against the exact Marcum value 7.916 dB (X), so the
  exact noncoherent integration loss at `n = 10`, `P_d = 0.9`, `P_fa = 1e-6` is
  `10 − 7.916 = 2.08 dB` (X). A recalled closed form gives 1.80 dB there and must not be used
  unchecked (R).
- **Fluctuation loss**, the extra SNR a Swerling 1 target needs over a steady one: the
  detectability page gives 8.1696 dB at `n = 10` (V), but that mixes two families: Barton's
  universal equation for Swerling 1 less Shnidman's Swerling 0
  (13.5033 − (13.1217 − 7.7881) = 8.1697 dB, computed here). Shnidman's equation for both gives
  8.2469 dB (X), and the exact value is 13.4996 − 5.2675 = 8.2321 dB (X).


### 2.5 Line of sight and terrain masking

**Horizon.** Over a smooth sphere with effective radius `k_e R_e` (standard refraction,
`k_e = 4/3`), the horizon range from height `h` is `d = √(2 k_e R_e h + h²)` (V, MathWorks
`horizonrange`). With `R_e = 6371 km`, `d ≈ 4.12 √h` km for `h` in metres (V-calc), and two
antennas see each other to `4.12 (√h₁ + √h₂)` km.

**At game scale the horizon does not matter.** A 30 m mast sees 22.6 km to a ground target
(V-calc), while the map at one metre a voxel is 384 m across (item 138 (4) (e)). Earth curvature
starts to matter only if the metric scale makes the map tens of kilometres across. So the lab
models curvature as an option, and the terrain is what masks.

**Terrain masking over a heightmap.** The lab casts a ray from the antenna phase centre to each
target point and walks the heightmap cells it crosses with the Amanatides–Woo traversal (Amanatides
and Woo 1987; a grid-walking algorithm, deterministic, one cell at a time), declaring the target
masked if any cell's surface height (plus the earth-curvature bulge, when enabled) rises above the
ray at that cell. There is no published golden for "a given heightmap masks a given point"; the
lab's tests here are constructed cases with an analytic answer (a single ridge of known height,
a flat plain, a bowl), stated as such, plus the horizon cases L-1 to L-3.

**A heightmap is a 2.5D simplification.** Item 138 (4) (f) asked for terrain masking over voxels.
A heightmap holds one surface height per column, so it loses overhangs and Mine's tunnels: a
target under an overhang reads as exposed, and a tunnel mouth as solid ground. Phase 2 works on the
heightmap, which is what an exported map and every constructed case need, and says so in its code.
Before any masking result is ported into the sim, the lab adds the 3D Amanatides–Woo traversal over
a voxel grid, the algorithm's own setting, and checks the port against it, so that what the sim
inherits is the voxel answer and not the heightmap's.

**Multipath (propagation factor), only because it is cheap.** Over a flat, perfectly reflecting
surface (reflection coefficient −1), the one-way pattern-propagation factor is
`F = 2 |sin(2π h_t h_r / (λ R))|`, and received power scales with `F⁴` (R: Skolnik ch. 8, Blake
1986). It is one closed form per target, so it costs nothing, but over a voxel heightmap the
reflecting surface is ill-defined. **Recommendation: model it as an optional flat-earth factor, off
by default,** and leave curved-earth propagation (MathWorks `radarpropfactor`, whose example was only
partly specified) and diffraction out of the lab until an experiment asks for them.

### 2.6 Land clutter

**Constant-gamma model** (V, MathWorks `constantGammaClutter`): `σ⁰ = γ sin ψ`, where `ψ` is the
grazing angle and `γ` a terrain constant, scaled with frequency as `γ = γ₀ + 5 log10(f / 10 GHz)`
dB (V, MathWorks `surfacegamma`). The clutter RCS of a resolution cell is `σ_c = σ⁰ A_c`, with
`A_c` the pulse-limited (`R θ_az (cτ/2) sec ψ`) or beam-limited cell area (R: Skolnik ch. 7,
Richards ch. 2).

Typical `γ` values (Barton's, at 10 GHz; their primary source is Barton 1985, "Land clutter models
for radar design and analysis", which this research did not read):

| Terrain | γ | Flag |
|---|---|---|
| Mountains, urban | −5 dB | Urban **V-calc**: MathWorks `landreflectivity("Urban", 20°)` gives σ⁰ = 0.1082 (V), and `10^(−0.5) sin 20° = 0.1082` (computed here). Mountains R. |
| Wooded hills | −10 dB | R |
| Rolling hills | −12 dB | R |
| Farmland, desert | −15 dB | R |
| Flatland | −20 dB | V (the MathWorks MTI example uses it) |
| Smooth | −25 dB | R |

The constant-gamma model is a fit for moderate grazing angles; at the low grazing angles a ground
radar on a voxel map would mostly see, Billingsley 2002 is the measured reference, and his
statistical models are a later lab experiment. No open, fully worked clutter-to-noise example was
found (section 7.9).

## 3. Doppler

### 3.1 Monostatic Doppler shift

`f_d = 2 v_r / λ`, with `v_r` the radial velocity (positive closing). It is the monostatic case of
the bistatic relation `f = −(1/λ) d(R_T + R_R)/dt` (V, section 4.1) with `R_T = R_R`. MathWorks'
`speed2dop` is the one-way shift `v/λ`: 35.76 m/s at 24.15 GHz gives 2880.7 Hz (V; V-calc
2880.67 Hz), so the lab's two-way function returns twice that for the same input.

**At game scale.** The game's walkers move one to two voxels a second (item 90). At one metre a
voxel and X-band (10 GHz, λ ≈ 3 cm) that is a Doppler shift of about 67 to 133 Hz (computed here),
small against a typical PRF but resolvable with a coherent processing interval of a few tens of
milliseconds. Item 138 (4) (e)'s point stands: whether such targets clear a ground radar's MDV
depends on the clutter spectrum width and the CPI, which the lab is for.

### 3.2 Pulse-Doppler, PRF and blind speeds

A pulsed radar samples Doppler at the PRF, so Doppler is ambiguous modulo the PRF, and an MTI
filter's nulls repeat at multiples of it: the **blind speeds** `v_b = n λ PRF / 2` (R: Skolnik
ch. 3, Richards ch. 5). Low PRF is range-unambiguous and Doppler-ambiguous, high PRF the reverse,
medium PRF ambiguous in both and resolved by several PRFs. **Staggered PRF** moves the first blind
speed out: MathWorks' staggered-PRF MTI example uses PRFs corresponding to unambiguous ranges of
6000 m and 5000 m (`c/(2·6000)` and `c/(2·5000)`, a 5:6 ratio), which moves the first blind Doppler
from about 30 kHz to 150 kHz (V); exactly, from `c/(2·5000)` = 29.979 kHz to
`6 · c/(2·6000) = 5 · c/(2·5000)` = 149.896 kHz (V-calc). Its carrier frequency is not printed, so
its velocities are only partly specified (section 7.9), but the blind-Doppler result depends only
on the PRFs and is a golden (M-3).

### 3.3 MTI and the minimum detectable velocity

The single delay-line canceller `[1 −1]` has `|H(f)| = 2 |sin(π f_d T)|`, the double canceller
`[1 −2 1]` the square of it; their clutter improvement factors for a Gaussian clutter spectrum of
standard deviation `σ_c` are about `2 (f_r / (2π σ_c))²` and `2 (f_r / (2π σ_c))⁴` (R: Skolnik
ch. 3, Richards ch. 5). The **minimum detectable velocity** is the radial speed at which the
filter's response has risen enough that the target's SNR, after the filter's loss, still meets the
detection threshold against the clutter residue; it is set by the clutter width, the filter and the
CPI. The MathWorks detectability page tabulates MTI losses of 1.4468 and 8.1562 dB for its radar
(V), but the inputs were not recorded, so those go to section 7.9. The canceller responses
themselves are exact formulae the lab tests by construction (a tone at a known Doppler through the
filter).

### 3.4 Range-Doppler processing

The range-Doppler map is the matched filter in fast time (one pulse) followed by an FFT across
pulses in slow time (the CPI). Range resolution is `c / (2B)`; Doppler resolution is `1 / T_CPI`.
The ambiguity function `|χ(t, f_d)| = |∫ x(u) e^{j2π f_d u} x*(u − t) du| / E_x` (V, MathWorks
`ambgfun`; Levanon and Mozeson 2004) describes what one waveform resolves. The MathWorks basic
monostatic radar example (case R-6) shows the range-gate quantisation the lab must reproduce:
targets at 2025, 3519 and 3845 m are estimated at 2025, 3525 and 3850 m (V). All three estimates lie
on a 25 m grid, which is what fast-time sampling at `2B` gives for its 50 m resolution
(`c/(2 · 2B) = 25 m`, computed here); the page's sampling rate is in section 7.9 to be read before
that becomes a golden.

## 4. Bistatic and passive radar with an illuminator of opportunity

### 4.1 Bistatic geometry and Doppler

With the transmitter and receiver separated by the baseline `L`, a target at ranges `R_T` from the
transmitter and `R_R` from the receiver lies on an ellipse of constant range sum; the measured
**bistatic range** is `R_T + R_R − L` (V). The **bistatic angle** `β` is the angle at the target
between the two lines of sight, and range resolution degrades with it:
`ΔR = c / (2 B cos(β/2))` (V, Pető and Seller 2017, eq. 1). The bistatic Doppler shift is
`f = −(1/λ) d(R_T + R_R)/dt` (V); the closed form `f_B = (2v/λ) cos δ cos(β/2)`, with `δ` the angle
between the target's velocity and the bistatic bisector, is consistent with it (Willis 2005; the
closed form's page is R, and the lab derives it from the V form).

### 4.2 The bistatic radar equation

`SNR = P_t G_t G_r λ² σ_B / ((4π)³ k T_s B R_T² R_R² L)` times the processing gain. Contours of
constant SNR are the **ovals of Cassini**, `R_T R_R = constant` (V). The power density at the
target is `Φ = P_t G_t / (4π R_T²)` (V). MathWorks' bistatic `radareqsnr` example is case B-1.

For passive radar the processing gain is the time-bandwidth product **`G_p = B · T`**: 1 s of a
50 kHz FM signal gives 47 dB (V, Howland, Maksimiuk and Reitsma 2005; three sources agree).

### 4.3 The cross-ambiguity function and range-Doppler maps

A passive receiver records a **reference channel** (the direct signal) and a **surveillance
channel** (echoes plus the direct signal leaking in). The **cross-ambiguity function**
`χ(τ, f) = ∫ s(t) r*(t − τ) e^{−j2π f t} dt` over the integration time is a matched filter against
a waveform the receiver did not choose; its magnitude over a grid of bistatic delay and Doppler is
the range-Doppler map. It is computed in batches (the integration time cut into segments, each
correlated by FFT, and an FFT across segments), the method in Howland 2005, Kuschel, Cristallini and
Olsen 2019, and Palmer et al. 2013 for DVB-T. Howland's FM system used 1 s integrations and a
50 kHz bandwidth, giving a range resolution of about 2–3 km and a Doppler resolution of about 1 Hz,
decimated to `f_s = 195.313 kHz`, with CA-CFAR over `M = 10` cells and a 3 dB threshold (all V).
At 96.8 MHz a 1 Hz Doppler cell is a bistatic range-rate resolution of `λ · Δf` = 3.1 m/s, or
1.5 m/s as the monostatic equivalent `λ · Δf / 2` (computed here).

### 4.4 Direct-path interference and its cancellation

The direct signal is far stronger than any echo: up to **90 dB** above a 10 m² echo at 150 km in
Howland 2005 (V), whose 1 s integration puts the CAF's sidelobes only 47 dB down (V), so an
uncancelled direct path buries every target. Cancellation:

- **Adaptive filters** (lattice or NLMS): Howland used an `M = 50` lattice/NLMS canceller, with no dB
  figure reported (V).
- **ECA, the Extensive Cancellation Algorithm** (Colone et al. 2009): a least-squares projection of
  the surveillance signal onto the subspace orthogonal to delayed (and Doppler-shifted) copies of the
  reference, removing the direct path and multipath clutter together. FFI report 14-01160 reports
  about 10–20 dB SNR improvement from ECA on DVB-T (S).
- **How much is needed.** O'Hagan, Baker and Griffiths 2006 (V): direct-path interference of
  −26 dBm in 300 kHz against a noise floor of −97 dBm (noise figure 25 dB, bandwidth 150 kHz) needs
  about 70 dB of suppression, of which analogue suppression achieves about 30 dB at best. The paper
  carries two noise figures for two scenarios: 25 dB with this interference analysis, whose
  −97 dBm floor it reproduces (−97.2 dBm, computed here), and 4.5 dB with the detection-range
  budget of case P-3, whose −148 dBW floor it reproduces (−147.7 dBW). Where in the paper each
  sits was not recorded (section 7.9). Spatial
  beamforming adds some: Pető and Seller 2018 report SINR gains of 3.9, 4.5, 5.4 and 5.2 dB (V),
  and cite Di Lallo at 13–25 dB (a secondary citation).

The lab models the direct path as a scenario input (its level relative to the noise floor) and
implements ECA, so the residual after cancellation is a measured output, not an assumption.

### 4.5 Illuminators of opportunity

Griffiths' NATO lecture note (Griffiths 2010, Table 1; V, reconstructed from a garbled text
extraction, so the reconstruction is itself flagged):

| Illuminator | Bandwidth | `P_t G_t` | Range resolution, `c/(2B)` | Power density at 100 km |
|---|---|---|---|---|
| HF | 9–10 kHz | 50 MW | | |
| FM | 50 kHz | 250 kW | 3000 m (V) | −57 dBW/m² (V; V-calc −57.0) |
| Analogue TV | 5.5 MHz | 1 MW | 30 m (V) | −51 dBW/m² (V; V-calc −51.0) |
| DAB | printed as 220 kHz; probably an extraction error for 1.536 MHz | 10 kW | | |
| DTV | 6 MHz | 8 kW | | |
| GSM | 200 kHz | 100 W | | |
| 3G | 3.84 MHz | 100 W | | |

Pető and Seller 2017, Table 1 (V; 0 dBi antennas, a 10 km baseline, `kT_sB` with `T_s = 300 K`),
which reproduces to within 0.9 dB (V-calc); the GSM floor computes to −120.8 dBm and is printed
−120, truncated, and the FM direct power computes to −12.4 dBm:

| Illuminator | EIRP | Bandwidth | Received direct power | Noise floor |
|---|---|---|---|---|
| FM | 100 kW | ~75 kHz | −12 dBm | −125 dBm |
| GSM | 100 W | 200 kHz | −61 / −67 dBm | −120 dBm |
| UMTS | | 3.84 MHz | −59 dBm | −108 dBm |
| DAB | 10 kW | 1.536 MHz | −29 dBm | −112 dBm |
| DVB-T | 1–100 kW | 7.61 MHz | −28 dBm | −105 dBm |
| DVB-S | | | | −99 dBm |

The lab's scenario names an illuminator by these parameters (EIRP, centre frequency, bandwidth,
and a signal model: band-limited noise is the honest default for DVB-T and a fair one for
programme-dependent FM), never by a table built into the code. Phase 4 uses these tables' values
only as fixtures for bare link budgets; terrestrial illuminator entries (EIRP, position, height)
and the antenna configurations that receive them arrive with phase 6 (section 9).

**ERP is used as EIRP where the source does so.** Hirsch and Kidd (case B-3) and O'Hagan (case P-3)
state an ERP and use it as `P_t G_t`. ERP is referred to a half-wave dipole and EIRP to an
isotropic antenna, so EIRP = ERP + 2.15 dB; converting would move B-3 to −73.9 dBW/m² and fail it
(computed here). The cases use ERP as EIRP, as the sources do, and say so.

### 4.6 Issue: MathWorks' passive bistatic example drops its noise figure

MathWorks' "Passive Bistatic Radar Performance Assessment" example states `P_t G_t = 100 kW`,
`G_r = 0 dB`, `σ = 10 m²`, `λ = 3 m`, `B = 50 kHz`, `T = 1 s`, `L = 10 dB`, a **5 dB noise
figure**, `T₀ = 290 K` and a 12 dB detection threshold, and publishes `K = 230.5413 dB` with
`SNR = K / (R_T² R_R²)` (V). The page's code never passes the noise figure to `radareqsnr`.
Including it gives **`K = 225.54 dB`** (X; 225.5413 dB computed here, in linear terms and as a
decibel sum). The equal-range 12 dB detection distance is then about **218 km** (X), against the
**291 km** the published `K` implies (computed here). The lab's golden is the corrected
225.54 dB, and the case records the published value and the reason it differs (case P-1).

## 5. Solar-illuminated passive radar

### 5.1 What the published work does

The published work uses the Sun's (or Jupiter's) broadband radio emission as a **noise
illuminator** and detects strong echoes from **large surfaces**: the bed of an ice sheet, a sea
surface, a subsurface ocean.

- **Romero-Wolf et al. 2015** (V): the concept paper, for Jupiter's icy moons, with Jupiter's
  decametric emission (not the Sun) as the source. The receiver sees the direct emission and its
  echo in one channel and finds the echo's delay by **autocorrelation**. Detection uses the
  radiometer equation `δT = T_sys / √(Δf Δt)` (eq. 22, V) and needs
  `√(Δf Δt) > N / √V` (eq. 24), i.e. `Δt > N² / (Δf V)` (eq. 25), with `V` the reflected-to-direct
  power factor and `N = 5`. Its worked example: 30 MHz, `Δf = 3 MHz`, ice 10 km thick, attenuation
  length 3 km, `N = 5`, gives `Δt ≈ 130 ms` (V), capped at about 0.7 s by spacecraft motion.
  Antenna temperatures from Jupiter's emission: 2.6 × 10⁹ K at Europa, 2.0 × 10⁸ K at Callisto,
  against a galactic background of about 6 × 10⁴ K (V). Depth resolution about 50 m at 3 MHz (V).
  **Caution:** its eq. 26 and eq. 9 disagree in the sign of the attenuation exponent
  (`e^{+2d/λ_A}` against `e^{−2D/λ_A}`); check before using either (section 7.9).
- **Peters et al. 2018**, IEEE TGRS (metadata V): a cliff-top prototype records the Sun's direct
  signal and its reflection from the ocean, and recovers the echo by autocorrelation. Its band,
  bandwidth, integration time and SNR were not verified (closed access).
- **Peters et al. 2021**, GRL (V): passive sounding of Store Glacier, Greenland, with the Sun,
  from 8 s acquisitions in the 2018 field season. Press reports, not the paper, give a 200–400 MHz
  band, an echo delay of 10.8 µs and ice about 900 m thick (unverified; a 1008 m figure in one press
  report is suspect).
- **Peters et al. 2021**, IEEE TGRS: passive synthetic-aperture imaging with radio-astronomical
  sources. **Schroeder et al. 2016**, PSS: the concept's surface-clutter analysis, whose SNR
  expression `SNR = 2√(hλ) β / [v (1 + √(h/λ) tan σ_s)²]` is quoted as eq. 11 in Romero-Wolf 2023
  (V). Further: IGARSS 2019; IEEE TGRS 62 (2024) 4302612 (S).

**Issue: nothing published detects a vehicle-sized target with the Sun as illuminator.** The
research found no paper that detects aircraft or vehicles this way. The nearest relative, passive
radar with GPS satellites as illuminators, detects only large targets with large arrays and long
integration (S).

### 5.2 Solar radio flux density

Solar flux density is measured in **solar flux units**: 1 sfu = 10⁻²² W m⁻² Hz⁻¹ (V). The
benchmark is F10.7, the flux at 2800 MHz, which runs from below 50 to above 300 sfu over the solar
cycle (V, NOAA SWPC; Tapping 2013, S) and varies day to day: 92 sfu on 2026-10-04 and 136 sfu on
2026-10-13 (V). The SWPC readings for 2026-10-14 by band (V), in sfu:

| MHz | San Vito | Learmonth |
|---|---|---|
| 245 | 15 | |
| 410 | 31 | |
| 610 | 57 | |
| 1415 | 83 | 78 |
| 2695 | 133 | 108 |
| 4995 | 208 | 174 |
| 8800 | 258 | 265 |
| 15400 | 551 | 541 |

The quiet Sun is about 1 sfu at 100 MHz and about 10 sfu at 240 MHz (V, Kansabanik 2023). The
flux rises steeply with frequency over this range, so a solar illuminator is far stronger at
microwave than at VHF.

**Bursts make the illuminator wildly variable.** The 2006-12-06 flare exceeded 10⁶ sfu (V, Gary
2019); type III bursts strengthen as frequency falls (Reid and Ratcliffe 2014); a "typical"
10²–10⁴ sfu burst figure is R. For the lab, the flux is a scenario input (a constant, a recorded
daily series, or a burst profile), never a constant in code.

### 5.3 Correlation and integration time

Two receiver designs appear:

- **One channel, autocorrelation** (the ice-sounding papers): direct and reflected emission arrive
  together, and the echo shows as a peak in the autocorrelation at its delay. Sensitivity follows
  the radiometer equation, so the required time goes as `N² / (Δf V)` (eq. 25 above): bandwidth
  buys time linearly, and a weak echo (`V` small) costs time as `1/V`.
- **Two channels, cross-correlation** (passive radar as in section 4): a reference antenna tracks
  the Sun, a surveillance antenna looks at the target area, and the CAF gives a bistatic
  delay-Doppler map, with the processing gain `B · T` (V for passive radar, section 4.2; the
  extension to a noise illuminator is R as a published rule, and the lab should verify it by
  simulation, which is exactly the kind of check it can make).

With the Sun effectively at infinity, the illumination at the target does not fall with range: the
echo falls only as `1/R_R²`, not `1/R⁴`. That is why short-range solar detection is less hopeless
than the ice-sounding geometry suggests.

### 5.4 What a sun-illuminated passive radar can and cannot detect

**Can (published):** strong specular echoes from large surfaces (an ice bed under hundreds of
metres of ice, a sea surface) with autocorrelation over seconds; imaging of large surfaces by
synthetic aperture.

**Cannot (published):** nothing published shows a vehicle or an aircraft. The reasons are visible
in the physics: the Sun's flux is small (about 10⁻²⁰ W m⁻² Hz⁻¹ at microwave), a vehicle's
cross-section is small against a surface's, the stationary ground illuminated by the same Sun
returns echoes at every delay (clutter, separable only by Doppler, so a stationary target is not
separable at all), and the Sun's radio output varies by orders of magnitude in a burst.

### 5.5 Integration time for a vehicle-sized target: an idealised estimate (computed here)

This estimate is **this memo's own arithmetic from published inputs**. It is not a published
result and is not a golden; it is what phase 5 of the lab should test by simulation.

Assumptions: the two-channel design of section 5.3; a target of `σ = 10 m²` (an assumed
vehicle-sized value, not a sourced one); a surveillance antenna of 1 m² effective area pointed at
the target with the Sun outside its beam; a reference antenna of 1 m² tracking the Sun; one
polarisation (half the unpolarised flux); `T_sys = 300 K`; `B = 10 MHz`; detection at the
steady-target 13.18 dB for `P_d = 0.9`, `P_fa = 1e-6` (section 2.3); processing gain `B · T`,
derated by the reference channel's own SNR as `SNR_ref / (1 + SNR_ref)` (derived here); solar flux
from section 5.2's table (the mean of the two stations' readings where there are two, rounded to
two figures: 80.5 to 80 and 120.5 to 120). Then the echo's spectral power is
`½ S σ A_e / (4π R²)` against `k T_sys`. The reference script recomputes the table (section 7.10).

| Band | Flux | 300 m | 1 km | 3 km | 10 km |
|---|---|---|---|---|---|
| 245 MHz | 15 sfu | 8.5 s | 94 s | 14 min | 2.6 h |
| 610 MHz | 57 sfu | 0.8 s | 9.3 s | 84 s | 16 min |
| 1415 MHz | 80 sfu | 0.5 s | 5.5 s | 50 s | 9 min |
| 2695 MHz | 120 sfu | 0.3 s | 3.0 s | 27 s | 5 min |

What it leaves out, each a lab experiment: **ground clutter** at the same delay, which a moving
target must clear by Doppler (and a stationary one cannot); **range migration** during coherent
integration (at 10 MHz the range cell is at least `c/(2B)` = 15 m, so a target moving at 2 m/s
leaves it after about 7 s, and longer integrations need migration compensation or noncoherent combination, which
loses gain); **the Sun's spatial coherence**: the Sun spans about 0.53°, so the echo and the
reference stay coherent only while the baseline across the line of sight, `d⊥`, keeps
`d⊥ · θ < c/B`, about 3.2 km at 10 MHz (computed here), which makes the 3 km and 10 km columns
optimistic while the 300 m and 1 km columns stand (see "Spatial Coherence Constraints on Passive
Radar Sounding With Radio-Astronomical Sources", IEEE TGRS 2024, DOI 10.1109/TGRS.2024.3456049);
**direct-path leakage** of the Sun into the surveillance antenna, which raises the noise floor, and
at 245 MHz a 1 m² aperture has only about 9 dBi of gain (computed here), a beam so broad that "the
Sun outside its beam" is doubtful in that band; the Sun's **variability and bursts**; and
**atmospheric and ash loss** in whatever band is chosen.

**What this means for item 138 (4).** Item 138 said that real solar passive radar "would not find
vehicle-sized targets within a 3-to-8-minute Push" and that a physically real version "would show
nothing on screen". The literature agrees that nobody has done it. The idealised budget above does
not rule it out at short range: at the map's present size (hundreds of metres, if a voxel is a
metre) and in the microwave bands, the time it needs is seconds, not minutes. Item 140 (4)
therefore amends item 138 (4)'s sentence to read: unpublished and untested; an idealised budget
does not rule it out at short range; phase 5 tests it. Whether the effects it leaves out close that
gap is the lab's first real question, and the answer depends on the metric scale (section 10).

### 5.6 The Sun's position

The lab needs the Sun's elevation and azimuth for a latitude, a longitude, a date and a time.
Four published algorithms, at different costs:

- **NREL's Solar Position Algorithm** (Reda and Andreas 2004, with its 2007 corrigendum): the
  reference, stated accuracy ±0.0003° for the years −2000 to 6000 (S). It has a fully specified
  worked example (case S-1) whose outputs are V through a published reproduction of NREL's test
  program; the table number in NREL's report was not read (section 7.9).
- **Michalsky 1988**, "The Astronomical Almanac's algorithm for approximate solar position
  (1950–2050)": stated accuracy 0.01° (V), with errata (1988, 1989) and Spencer's comment (1989)
  fixing a southern-hemisphere error in the azimuth. No worked example was found, so it is tested
  against SPA within its stated accuracy (case S-2).
- **The Astronomical Almanac's low-precision formula** as USNO publishes it (V):
  `D = JD − 2451545.0`; `g = 357.529 + 0.98560028 D`; `q = 280.459 + 0.98564736 D`;
  `L = q + 1.915 sin g + 0.020 sin 2g`; `R = 1.00014 − 0.01671 cos g − 0.00014 cos 2g`;
  `ε = 23.439 − 0.00000036 D`; `RA = atan2(cos ε sin L, cos L)`; `sin δ = sin ε sin L`, accurate to
  about 1′ within two centuries of 2000 (V). No worked example was found; a NASA transcription of it
  (`aa_sun.pro`) is inconsistent and must not be used.
- **NOAA's calculator** (V) follows Meeus (1998); NOAA's own low-precision fractional-year formulas
  (Spencer 1971) are in section 7.9 because only a reproduction of them was read.

**Recommendation:** SPA as the lab's reference, and the Almanac's low-precision formula as the cheap
second implementation that a later in-sim port would use, tested against SPA within 1′. A port to
the sim would need it in integer arithmetic over the angle LUT, and only if the owner wants a sun
at all (section 10).

## 6. The Pall and the band

The Pall is a sky of ash. Volcanic ash is a radar target in its own right: weather radars at C-band
have measured ash clouds, and forward models of ash backscatter have been run at S-, C-, X- and
Ka-band (Marzano, Vulpiani and Rose 2006a; Marzano, Barbieri, Vulpiani and Rose 2006b; S, from the
papers' abstracts). So "long

wavelengths pass; at X-band and above, ash shows as clutter and attenuates" (item 138 (4)) has
published support in direction, but this research did not read an attenuation figure per band. The
lab treats ash as a scenario input (an attenuation in dB/km and a volume-clutter reflectivity per
band), with no default until a value is read on a source page. Which band the game uses, and
whether the Sun stands above the Pall at all, are the owner's (section 10).

## 7. Validation cases: the lab's golden tests

Each case gives every input, the published output, the tolerance the lab tests to, where it comes
from, and its flag. **Only V, V-calc and X values are goldens.** Section 7.9 lists everything else
as "to verify". Where a source prints a value to a fixed number of decimals, the tolerance is one
unit in the last printed digit unless the case says otherwise; that covers rounding and the
difference between the 2019 SI value of `k` and older ones. Every case in 7.1 to 7.8 is **fully
specified** unless it says "partial". Where the research did not record the page, table or figure
a published value comes from, the case says "locator not recorded", and the locator goes in
section 7.9, to be recorded before the phase lands that golden.

Constants for every case unless it says otherwise: `c = 299 792 458 m/s`,
`k = 1.380649 × 10⁻²³ J/K`, `T₀ = 290 K`, and the MathWorks defaults where a MathWorks case leaves
a value out (`σ = 1 m²`, `T_s = 290 K`, `G = 20 dB`, `L = 0 dB`).

### 7.1 The radar range equation (phase 1a)

| Case | Inputs | Published output | Tolerance | Source | Flag |
|---|---|---|---|---|---|
| R-1 | f 1 GHz; R 50 km; P_t 1 MW; τ 0.2 µs; σ 1 m²; G_t = G_r 20 dB; T_s 290 K; L 0 dB | SNR 5.5868 dB | ±0.0001 dB | MathWorks `radareqsnr` reference page, example 1 | V, V-calc 5.58681 |
| R-2 | f 10 GHz; R 100 km; P_t 1 MW; τ 1 µs; σ 0.5 m²; G 40 dB; T_s 300 K; L 3 dB | SNR 14.3778 dB | ±0.0001 dB | same, example 2 | V, V-calc 14.37777 |
| R-3 | f 10 GHz; required SNR 6 dB; P_t 1 MW; τ 10 µs; defaults otherwise | R 4.1057 × 10⁴ m | ±5 m | MathWorks `radareqrng` reference page, example 1 | V, V-calc 41 056.6 m |
| R-4 | as R-3, plus σ 0.1 m², G 40 dB, L 3 dB | R 1.9426 × 10⁵ m | ±50 m | same, example 2 | V, V-calc 194 259.7 m |
| R-5 | f 3 GHz; P_t 0.2 MW; τ 11 µs; G 34 dB; T_s = `systemp(4.1)` = 745.41 K; σ 1 m²; L 0 dB | SNR at 100 km 18.3169 dB; range at which SNR equals D = 13.5033 dB: 131.9308 km | ±0.0005 dB; ±0.001 km | MathWorks, "Modeling radar detectability factors" | V; V-calc 18.3171 dB at exactly 100 km (the page samples range on a 40 m grid, which explains the 0.0002 dB gap) and 131.9305 km |
| R-6 | P_d 0.9; P_fa 1e-6; R_max 5000 m; ΔR 50 m (B = c/100, τ = 1/B); n = 10 noncoherent; f 10 GHz; G_t = G_r 20 dB; σ 1 m²; noise kT₀B, noise figure 0 dB | minimum SNR `albersheim(0.9, 1e-6, 10)` = 4.9904 dB; peak power 5.2265 × 10³ W; PRF ≈ 29.98 kHz | ±0.0001 dB; ±0.5 W; ±0.01 kHz | MathWorks, "Designing a basic monostatic pulse radar" | V, V-calc 4.99039 dB and 5226.47 W (5226.49 W from the rounded 4.9904 dB) |

### 7.2 Detection (phases 1b and 1c)

All at one pulse unless `n` is given. "Exact" means the square-law detector with Marcum Q or the
incomplete gamma function. Each function is tested **only against values produced by its own
formula** (section 2.3). Every X value below is printed by the reference script (section 7.10).

| Case | Function | Inputs | Published output | Tolerance | Source | Flag |
|---|---|---|---|---|---|---|
| D-1 | Swerling 1 closed form `P_d = P_fa^(1/(1+SNR))` | P_d 0.9; P_fa 1e-6 | 21.1436 dB | ±0.0001 dB | MathWorks detectability-factor page (D₁) | V; V-calc 21.1436 dB |
| D-2a | Shnidman, Swerling 0 | P_d 0.9; P_fa 1e-6 | 13.1217 dB | ±0.0001 dB | same (D₀) | V |
| D-2b | Exact Marcum, Swerling 0 | P_d 0.9; P_fa 1e-6 | 13.1835 dB | ±0.0005 dB | reference script, series and quadrature; numbers review | X |
| D-2c | Albersheim | P_d 0.9; P_fa 1e-6; n 1 | 13.1145 dB | ±0.0001 dB | Albersheim 1981 formula, evaluated | X |
| D-3a | Albersheim | P_d 0.9; P_fa 1e-6; n 10 | 4.9904 dB | ±0.0001 dB | MathWorks `albersheim`; case R-6 | V, V-calc 4.99039 |
| D-3b | Exact Marcum, Swerling 0 | P_d 0.9; P_fa 1e-6; n 10 | 5.2675 dB | ±0.0005 dB | reference script, series and quadrature; numbers review | X |
| D-4 | Albersheim | P_d 0.9; P_fa 0.01; n 1 | 9.5027 dB | ±0.0001 dB | Albersheim 1981 formula, evaluated; the `albersheim` page prints no number for these inputs. `P_fa` 0.01 lies outside the formula's stated validity range (R: `P_fa` 1e-7 to 1e-3), which is fine for a test of the formula and says nothing about accuracy | X |
| D-5 | Shnidman, Swerling 1 | P_d 0.9; P_fa 1e-8; n 10 | 14.7131 dB | ±0.0001 dB | Shnidman 2002 formula as the MathWorks `shnidman` page states it, evaluated; the page prints no number for these inputs | X |
| D-6a | Shnidman, Swerling 2 | P_d 0.9; P_fa 1e-6; n 10 | 6.1583 dB | ±0.0001 dB | Shnidman 2002 formula, evaluated (the inputs of the MathWorks phased waveform-design example) | X |
| D-6b | Exact, Swerling 2 | same | 6.2918 dB | ±0.0005 dB | reference script, closed form and quadrature; numbers review | X |
| D-7a | Shnidman, Swerling 1 | P_d 0.9; P_fa 1e-6; n 10 | 13.5805 dB | ±0.0001 dB | Shnidman 2002 formula, evaluated. The detectability page's 13.5033 dB for these inputs is Barton's universal equation, not Shnidman's (section 2.3); it waits in section 7.9 | X |
| D-7b | Exact, Swerling 1 | same | 13.4996 dB | ±0.0005 dB | reference script, closed form and quadrature; numbers review | X |
| D-8 | Shnidman integration gain and fluctuation loss | Swerling 0, n 1 → 10, P_d 0.9, P_fa 1e-6; Swerling 1 against 0 at n 10 | G_i 7.7881 dB; L_f 8.2469 dB | ±0.0001 dB | G_i: MathWorks detectability-factor page. L_f: Shnidman 2002 formula, evaluated (D-7a less Shnidman's Swerling 0 at n 10, 5.3336 dB). The page's 8.1696 dB is Barton less Shnidman and waits in section 7.9 | G_i V, V-calc 7.7881; L_f X |
| D-9 | Exact integration gain, integration loss and fluctuation loss | as D-8 | G_i 7.916 dB; integration loss 2.08 dB; L_f 8.2321 dB | ±0.001 dB; ±0.01 dB; ±0.001 dB | reference script; numbers review | X |
| D-10 | Coherent detection of a **complex** signal, `P_d = ½ erfc(erfcinv(2P_fa) − √SNR)` (MathWorks' `NonfluctuatingCoherent`) | SNR 8 dB; P_fa 0.01 | P_d 0.8899 | ±0.0001 | formula from MathWorks `rocsnr`, evaluated; the page prints no 0.8899 | X (closed form and quadrature agree) |
| D-11 | Coherent threshold, real signal, `20 log10 Q⁻¹(P_fa)` | P_fa 0.01; P_fa 1e-6 | 7.3335 dB; 13.5401 dB | ±0.0001 dB | MathWorks `npwgnthresh` | 7.3335 V, V-calc 7.33349; 13.5401 X |

The Marcum Q implementation (phase 1b) also gets the identities that hold exactly: `Q₁(0, b) = e^{−b²/2}`,
monotonicity in both arguments, and `Q + (1 − Q) = 1` computed from the two separately-evaluated
tails to 1e-12 over a grid down to `P_fa = 1e-12`.

### 7.3 Line of sight and horizon (phase 2)

| Case | Inputs | Published output | Tolerance | Source | Flag |
|---|---|---|---|---|---|
| L-1 | h 30 m; target at 0 m; k_e 4/3; R_e 6371 km; `d = √(2 k_e R_e h + h²)` | 22.6 km | ±0.05 km | MathWorks `horizonrange` | V, V-calc 22.576 km |
| L-2 | h_R 30 m; target at 1 km altitude; `4.12 (√h₁ + √h₂)` km | 153 km | ±0.5 km | O'Hagan, Baker and Griffiths 2006; locator not recorded | V, V-calc 152.85 km (computed here) |
| L-3 | h_T 313 m; target at 1 km altitude | 203 km | ±0.5 km | same; locator not recorded | V, V-calc 203.18 km (computed here) |

Terrain masking over a heightmap has no published golden; its tests are constructed with analytic
answers (section 2.5) and say so.

### 7.4 Clutter (phase 2)

| Case | Inputs | Published output | Tolerance | Source | Flag |
|---|---|---|---|---|---|
| C-1 | Barton land model, urban, 10 GHz, grazing 20° (constant gamma, γ = −5 dB) | σ⁰ = 0.1082 | ±0.0001 | MathWorks `landreflectivity("Urban", 20°)` | V; V-calc `10^(−0.5) sin 20° = 0.10816` (computed here) |
| C-2 | `surfacegamma` frequency scaling `γ = γ₀ + 5 log10(f / 10 GHz)` | the formula; test at f = 10 GHz (γ = γ₀) and a decade below (γ₀ − 5 dB) | exact | MathWorks `surfacegamma` | V (formula) |

### 7.5 Doppler and MTI (phase 3)

| Case | Inputs | Published output | Tolerance | Source | Flag |
|---|---|---|---|---|---|
| M-1 | one-way Doppler, v 35.76 m/s, f 24.15 GHz | 2880.7 Hz (the lab's two-way function: twice this) | ±0.1 Hz | MathWorks `speed2dop` | V, V-calc 2880.67 Hz |
| M-2 | two-way monostatic Doppler as the special case `R_T = R_R` of `f = −(1/λ) d(R_T + R_R)/dt` | identity | 1e-12 relative | derived from the V bistatic form | V (form) |
| M-3 | staggered PRFs with unambiguous ranges 6000 m and 5000 m (PRFs `c/(2·6000)` and `c/(2·5000)`, ratio 5:6) | first blind Doppler moves from ≈ 30 kHz to 150 kHz; exactly, from 29.979 kHz to `6 · c/(2·6000) = 5 · c/(2·5000)` = 149.896 kHz | published figures ±0.5 kHz; exact values ±0.001 kHz | MathWorks staggered-PRF MTI example | ≈ 30 and 150 kHz V; 29.979 and 149.896 kHz V-calc; **partial** (the example's carrier frequency is not printed, so only the Doppler result, not a velocity, is a golden) |
| F-1 | the in-crate FFT | impulse, single tone, Parseval, agreement with a direct DFT at N = 1024 | 1e-12 relative | mathematical identities | not a published case; stated as such |

### 7.6 Bistatic and passive (phases 4a, 4b and 6)

| Case | Inputs | Published output | Tolerance | Source | Flag |
|---|---|---|---|---|---|
| B-1 | bistatic `radareqsnr`: f 10 GHz; R_T 50 km; R_R 75 km; P_t 1 MW; τ 1 µs; G_t 40 dB; G_r 20 dB; σ 1 m²; T_s 290 K; L 0 dB | SNR 9.0547 dB | ±0.0001 dB | MathWorks `radareqsnr`, example 3 | V; V-calc 9.05468 dB (computed here) |
| B-2 | FM, P_tG_t 250 kW, 100 km; analogue TV, 1 MW, 100 km; power density `P_tG_t/(4πR²)` | −57 dBW/m²; −51 dBW/m² | ±0.5 dB | Griffiths 2010, Table 1 and text | V; V-calc −57.01 and −50.99 (computed here) |
| B-3 | 103.5 MHz, ERP 50 kW used as EIRP, as the source does (section 4.5), 400 km | −76 dBW/m² | ±0.5 dB | Hirsch and Kidd 2013; locator not recorded | V-calc −76.04 |
| B-4 | range resolution `c/(2B)`: FM 50 kHz; TV 5.5 MHz | published 3000 m and 30 m; exact 2997.92 m and 27.25 m | published ±3 m for both (the source rounds: 2997.92 m to 3000, 27.25 m to 30); exact ±0.01 m | Griffiths 2010 | published V; exact V-calc |
| P-1 | MathWorks passive bistatic example: P_tG_t 100 kW; G_r 0 dB; σ 10 m²; λ 3 m; B 50 kHz; T 1 s; L 10 dB; F 5 dB; T₀ 290 K; threshold 12 dB | **corrected** K = 225.54 dB and equal-range 12 dB distance ≈ 218 km; published K = 230.5413 dB and ≈ 291 km, which omit the noise figure (section 4.6) | ±0.01 dB; ±1 km | MathWorks, "Passive Bistatic Radar Performance Assessment" | published value V; corrected value X (225.5413 dB and 218.0 km, reference script) |
| P-2 | Howland 2005 processing gain: B 50 kHz, T 1 s | G_p = 47 dB | ±0.1 dB | Howland, Maksimiuk and Reitsma 2005; locator not recorded | V, V-calc 46.99 dB |
| P-3 | O'Hagan 2006: ERP 4 kW (36 dBW), used as EIRP, as the source does; G_r 2.2 dBi; λ 3 m; σ_B 100 m²; B 150 kHz; F_n 4.5 dB (the detection-range scenario, section 4.4); L 0 dB; SNR 10 dB | noise floor −148 dBW (V-calc −147.7); `R_T R_R` at 10 dB = 4.21 × 10⁸ m² (≈ 20.5 km at equal ranges); with G_p 47 dB, ≈ 307 km at equal ranges | ±0.5 dB; ±1 % | O'Hagan, Baker and Griffiths 2006; locator not recorded | inputs V; noise floor V-calc; the two range figures X. **Partial:** the paper's own outputs (about 25 km, and 365 km with processing gain) are read off figures, and the 365 km contour is not identified, so they are not goldens |
| P-4 | noise floors `k · 300 K · B`: FM 75 kHz; GSM 200 kHz; UMTS 3.84 MHz; DAB 1.536 MHz; DVB-T 7.61 MHz. Direct power, FM 100 kW EIRP at 10 km, 0 dBi, λ 3 m | −125, −120, −108, −112, −105 dBm; −12 dBm | ±0.5 dB, except GSM ±1 dB (the source truncates −120.8 to −120) | Pető and Seller 2017, Table 1 | V; V-calc −125.1, −120.8, −108.0, −112.0, −105.0 and −12.4 dBm (computed here) |

**Phase 4 and phase 6 use these cases differently** (section 9). Phase 4 uses them only as fixtures
that validate the passive chain: B-1 to B-4 and P-1 to P-4 as bare link budgets, and P-2's
processing gain checked again by the cross-ambiguity function on a synthetic signal. Phase 6 runs
the passive cases (B-2 to B-4 and P-1 to P-4: the MathWorks passive example, O'Hagan, Howland, and
Pető and Seller) through the receiver site of section 8.6, with their transmitters as terrestrial
illuminator entries, rather than through a bare equation.

### 7.7 Solar position (phase 5)

| Case | Inputs | Published output | Tolerance | Source | Flag |
|---|---|---|---|---|---|
| S-1 | 2003-10-17 12:30:30 local, timezone −7 h; ΔUT1 0; ΔT 67 s; longitude −105.1786°; latitude 39.742476°; elevation 1830.14 m; pressure 820 mbar; temperature 11 °C; surface slope 30°; surface azimuth rotation −10°; atmospheric refraction at sunrise and sunset 0.5667° | JD 2452930.312847; heliocentric L 24.01826°, B −0.0001011219°, R 0.996542 AU; observer hour angle H 11.105902°; Δψ −0.003998404°; Δε 0.001666568°; ε 23.440465°; topocentric zenith 50.111622°; topocentric azimuth 194.340241°; surface incidence 25.187000°; sunrise 06:12:43; sunset 17:20:19 local | one unit in the last printed digit; ±1 s for sunrise and sunset | Reda and Andreas 2004 and NREL/TP-560-34302, via the published `spa_tester` output | V (through a published reproduction of NREL's test program); the table number in NREL's report is **not verified**. Sunrise, transit and sunset are goldens as **faithful to SPA's appendix A.2 procedure**, not as physical truth: the numbers review's independent geometry gives sunrise within 3 s, transit 11:46:06 and sunset 17:18:52, 87 s before SPA's 17:20:19, a discrepancy that stands until NREL's report is read (section 7.9) |
| S-2 | Michalsky 1988 at S-1's place and time, and over a year of daylight hours at S-1's place and at a southern-hemisphere place | agrees with SPA within 0.01° (Michalsky's stated accuracy) in **unrefracted** elevation and in azimuth, with Spencer's 1989 azimuth fix applied. Refracted elevations are not compared: Michalsky's refraction ignores pressure while SPA scales it by `P/1010` (0.81 at 820 mbar), so near the horizon the two differ by about 0.1° (numbers review) | 0.01° | Michalsky 1988; accuracy statement in the `solposx` documentation | V (accuracy statement); not a single published output |
| S-3 | USNO's Almanac low-precision formula at the same hours | RA and declination agree with SPA's geocentric values within 1′ | 1′ (0.0167°) | USNO, "Approximate solar coordinates" | V (accuracy statement) |

### 7.8 Solar illumination (phase 5)

| Case | Inputs | Published output | Tolerance | Source | Flag |
|---|---|---|---|---|---|
| N-1 | radiometer equation `δT = T_sys / √(Δf Δt)` and the detection condition `Δt > N² / (Δf V)` | the formulae (eq. 22, 24, 25) | exact | Romero-Wolf et al. 2015 | V |
| N-2 | 1 sfu | 10⁻²² W m⁻² Hz⁻¹ | exact | NOAA SWPC; locator not recorded | V |

There is **no published golden for a solar echo from a vehicle-sized target**, because no such
detection is published (section 5.1). Section 5.5's table is computed here and is what phase 5's
simulation tests, not a golden.

### 7.9 To verify (S and R, and partial cases)

None of these may become a golden until a V or V-calc value replaces it.

| Item | Why it is here | What would verify it |
|---|---|---|
| Albersheim's ≈ 0.2 dB and Shnidman's ≈ ±0.5 dB accuracy statements, and Albersheim's validity range (`P_fa` 1e-7 to 1e-3) | R | Albersheim 1981 and Shnidman 2002, read on the page |
| The detectability page's Swerling 1 values at `n = 10`, `P_d = 0.9`, `P_fa = 1e-6`: 13.5033 dB, and the fluctuation loss of 8.1696 dB (Barton's Swerling 1 less Shnidman's Swerling 0) | V-page values of Barton's universal equation, which the lab does not implement until the equation is read on the page | Barton 2013, read on the page; then a `barton_universal()` function takes them as its goldens |
| Locators (page, table or figure) for L-2, L-3 and P-3 (O'Hagan, Baker and Griffiths 2006, including where each of its two noise figures sits), B-3 (Hirsch and Kidd 2013), P-2 (Howland, Maksimiuk and Reitsma 2005) and N-2 (NOAA SWPC) | not recorded by the research | the sources, read in place |
| S-1's sunset: SPA's 17:20:19 against the numbers review's independent 17:18:52 | the review's geometry agrees on sunrise to 3 s and gives transit 11:46:06 | NREL/TP-560-34302's appendix A.2, read in place |
| Swerling 1 to 4 definitions | R | Swerling 1960, or Richards 2014 ch. 6, or Skolnik 2001 ch. 2 |
| Blake's system-noise-temperature decomposition and the NRL 6930 worksheet example | DTIC unavailable during the research | NRL Report 6930 (DTIC AD0701321), read in place |
| MTI losses 1.4468 and 8.1562 dB; binary integration 1.0549 dB; CFAR 0.2500 dB (P_fa 1e-6, 120 cells); beam-shape 1.2338 and 2.4677 dB; scan 2.7745 dB | V-page values, but their inputs were not recorded | the MathWorks detectability-factor page, inputs recorded |
| Barton's γ for mountains, wooded hills, rolling hills, farmland and desert, smooth | R (a search summary of a secondary source) | Barton 1985, "Land clutter models for radar design and analysis", the primary source; or Barton 2013, or Nathanson 1991 |
| Flat-earth multipath `F = 2 \|sin(2π h_t h_r/(λR))\|` and `F⁴` scaling | R | Skolnik 2001 ch. 8, or Blake 1986 |
| Blind speeds, canceller responses and improvement factors (section 3.3) | R | Skolnik 2001 ch. 3, or Richards 2014 ch. 5 |
| Range estimates 2025 / 3525 / 3850 m in the basic monostatic example | V; they lie on a 25 m grid, which fits fast-time sampling at `2B` (computed here), but the page's sampling rate was not recorded | the same MathWorks page |
| Bistatic Doppler closed form `(2v/λ) cos δ cos(β/2)` | R (consistent with the V form) | Willis 2005 |
| MathWorks curved-earth `radarpropfactor` example | partly specified | the page |
| Griffiths and Baker 2005, Part 1, illuminator table and worked detection ranges | full text not reached | the paper |
| Griffiths and Baker's 2nd edition, Willis and Griffiths 2007 | S | the books |
| ECA's 10–20 dB SNR improvement on DVB-T | S (FFI 14-01160) | the report |
| O'Hagan's 25 km and 365 km detection ranges | read off figures; the contour behind 365 km is not identified | the paper's text |
| Romero-Wolf eq. 26 against eq. 9 (sign of the attenuation exponent) and the reflected-to-direct factor behind the 130 ms example | the two equations disagree | the paper; recompute the example |
| Peters et al. 2018: band, bandwidth, integration time, SNR | closed access | the paper |
| Peters et al. 2021: band 200–400 MHz, echo delay 10.8 µs, ice ≈ 900 m | press reports only | the paper |
| Schroeder 2016's SNR expression | read only as quoted in Romero-Wolf 2023 | Schroeder et al. 2016 |
| Tapping 2013 on F10.7 | S | the paper |
| Burst magnitudes 10²–10⁴ sfu "typical" | R | a solar radio burst catalogue or review |
| SPA's ±0.0003° accuracy | S | Reda and Andreas 2004 |
| The NREL report's table number for the S-1 example | not read | NREL/TP-560-34302 |
| NOAA's fractional-year formulas (Spencer 1971) | read only through a reproduction | NOAA's "General Solar Position Calculations" note |
| Meeus example 25.a (1992 Oct 13.0 TD: true longitude 199°54′36.0″, R 0.99766 AU, apparent longitude 199°54′32.0″, RA 13h13m31.4s, declination −7°47′06.0″) | partial, through a software reproduction; Meeus' ISBN is R | Meeus 1998, read on the page |
| Ash attenuation and reflectivity by band | S | Marzano et al. 2006a and 2006b |
| A vehicle's RCS | not researched | a published measurement, if the lab ships a default |

### 7.10 The reference script

Appendix A holds the reproducible form of "computed here": a reference script printed in this memo
as a code block, not committed as a file. AGENTS.md §3 allows two languages in the tree, Rust and
GDScript, and nothing else executes; the script is documentation, so it lives as text in the memo,
and a reader copies it out to a file outside the repository to run it. It uses Python's standard
library only and is run isolated (`python -I radar-lab-reference.py`, about a minute). It prints
every X value in sections 7.2 and 7.6, the V-calc values it reproduces, section 5.5's table, and
the other figures this memo marks "computed here". The exact detection values are computed by two
methods inside it (a series or closed form, and quadrature of the density), and it exits with an
error if they disagree by more than 1e-4 dB. It is not the lab's implementation and not a golden:
the lab's goldens are the values in section 7, and the lab implements its own functions in Rust
from the published algorithms (section 8.4), whose phase 1 tests then hold the same values.

## 8. Architecture of the lab

### 8.1 The crate

- **`crates/radar-lab`, package `pharmakos-radar-lab`**, a library plus one binary, `radar-lab`
  (the binary name is a suggestion; `gamectl` stays the only shipped CLI). `publish = false`.
- **Walled.** Its name joins `WALLED_PACKAGES` in `xtask/src/main.rs`, with `clippy.toml`'s header
  and AGENTS.md §4.9 in step. That is a §5 contract change (it widens the float, cast, hash-map and
  clock allowance for the whole crate, which is why section 8.7 binds the lab's own numeric rules).
  The owner approved the crate and the contract changes this section names in principle on
  2026-10-14, leaving their exact shape to phase 0 (item 140 (3) (a)). Phase 0's pull request
  carries them out, names every contract path it touches, and opens and stops like any contract
  pull request.
- **It reaches nothing of the game.** It depends on no `pharmakos-*` crate at all, in any phase:
  the physics needs no game type, and a dependency on the sim would only add a way to get the wall
  wrong. Two checks hold it:
  - A `CLIENT_WALL` line, `("radar-lab", &["sim", "verifier", "plan-core", "gateway",
    "operator"])`, which `wall-guard` enforces, transitively included. `CLIENT_WALL`'s doc and
    AGENTS.md §4.9's bullet define the list as the client side of the wall, and the lab is not
    client-side, so phase 0 generalises both to "walled crates that must not reach the core". The
    list keeps its name, because `crates/client-gdext/tests/no_sim.rs` parses it.
  - That line forbids five core crates only; `pharmakos-proto`, the mesher and the client stay
    reachable through it. So a test in the lab reads `Cargo.lock` and asserts that the lab has no
    `pharmakos-*` dependency at all.

  It does **not** join `GUARDED_PACKAGES`: forbidding the sim already keeps `research` out of
  reach, and joining would only add the no-`[features]` rule and AGENTS.md §3 rule 1's count for
  no further guarantee (engineering call; you can veto). If phase 0 keeps the join as defence in
  depth after all, its contract paths grow by `GUARDED_PACKAGES`' doc comment (which says `bench`
  "is the one walled crate here"), the research guard's error text, the bench comments in
  `the_guard_lists_partition_the_crates`, and AGENTS.md §3 rule 1 ("any of the six", and the
  no-`[features]` rule). If a later phase wants the map generator's heightmap, it reads an exported
  heightmap file rather than linking the sim (see 8.3).
- **Nothing that ships reaches it, and phase 0 adds the check that says so.** `wall-guard`'s first
  relation fails any deterministic crate that reaches a walled one, which covers `gamectl`. Nothing
  covers the shipped client today: `cargo xtask package` builds `client-gdext --lib` and `gamectl`
  (`xtask/src/package.rs`, its `build`), `client-gdext` is itself walled so the first relation never
  checks it, and its `CLIENT_WALL` line cannot name the lab, because
  `the_client_wall_names_walled_crates_only` refuses a walled crate there, and `no_sim.rs`'s
  `FORBIDDEN` must equal that line. So phase 0 adds a relation to `wall-guard`: no package root
  (`gamectl`, `client-gdext`) reaches a never-shipped walled crate (`bench`, `radar-lab`),
  transitively included, with a test that a root reaching either is refused. It names `bench`
  outright as well: today the client reaching `bench` fails only because `bench` reaches the sim,
  which the client's line forbids. (Engineering call; you can veto. The alternative, a
  `Cargo.lock` test in `client-gdext` that admits only `pharmakos-proto` and `pharmakos-mesher`,
  would cover the client alone and put the check in a crate another lane may own.)
- **Not in the cross-OS guard.** Its float outputs are not compared byte for byte across operating
  systems (item 138 (4) (b)). Its goldens compare against published values with a stated tolerance,
  which is the same on every OS, so its fast tests run in the ordinary `test` step on all three
  legs. Its heavy simulations (cross-ambiguity batches, ECA over long records, the solar
  integration runs of section 5.5) are `#[ignore]`d checks run by hand, like
  `crates/gamectl/tests/balance.rs`: never a step, never compared across operating systems, so
  `cargo test --workspace` in debug on three operating systems stays fast.
- **Licence:** `GPL-3.0-or-later` like the rest of the game code (AGENTS.md §8), although it never
  ships (item 140 (3) (b)).

### 8.2 Floats: is f64 enough?

f64 (53-bit significand, about 15.9 significant decimal digits, range to about 1.8e308) is enough
for every model in this memo **if the code works in the log domain where the physics spans many
decades, and computes tail probabilities directly rather than as `1 − p`.** The places that need
care:

| Computation | Hazard | Treatment |
|---|---|---|
| Radar equation | Products such as `k T_s` (about 4e-21 W/Hz at 290 K, so `k T_s B` is about 4e-15 W in 1 MHz) against `P_t G² λ² σ` span about 40 decades; `R⁴` at 1e5 m is 1e20. | No overflow in f64, but sum in decibels (`10 log10`) for the budget tables, as every textbook does, and keep linear values only for the final SNR. |
| Pfa near 1e-6 to 1e-10 | `1 − Pfa` rounds to 1; a threshold from `−ln(Pfa)` is fine, from `ln(1 − (1 − Pfa))` is not. | Carry Pfa itself and `ln Pfa`; never form `1 − Pfa` and subtract back. |
| Marcum Q, Q_M(a, b), steady target | For large `a` and `b` the series terms overflow before they cancel; for `b ≫ a` the result underflows; for Pd near 1 the complement `1 − Q` is the meaningful quantity and cancels. | Use a published algorithm that is stable over the range: Shnidman's 1989 method for the detection probability, or Gil, Segura and Temme's 2014 algorithm, which selects series, asymptotic expansions or quadrature by region and returns both `Q` and `1 − Q`. Test against the published values and the Swerling closed forms (cases D-1 to D-11). |
| Incomplete gamma (Swerling 2 and 4, noncoherent integration) | Same cancellation in the upper tail. | Compute the regularised `Q(a, x)` and `P(a, x)` separately (Temme's or DiDonato and Morris's method), never one from the other near 0 or 1. |
| Cross-ambiguity function | An FFT of length 2^20 to 2^24 accumulates rounding of order `ε log2 N`, about 1e-15 × 24 in relative terms: far below the thermal noise floor it is used to find. | f64 throughout; f32 would also do but saves nothing that matters in a lab. |
| Solar ephemeris | Julian day near 2.46e6 with a fraction of a second needs about 1e-11 relative precision. | f64 has it; keep the day number split as (whole days since J2000, fraction) as NREL's SPA does in effect, so that angles reduced modulo 360° keep their precision. |
| Very small cross-correlation SNR (−60 to −90 dB) | None: these are ratios carried in dB or as f64 down to 1e-308. | None needed. |

So: **f64 everywhere, no extended precision crate.** The hazards are cancellation, not range, and
they are handled by algorithm choice, each pinned by a golden.

### 8.3 Inputs and outputs

- **In: a scenario file**, JSON that may carry `//` line comments, parsed by a hand-written strict
  decoder in the crate (not `plan-core`'s JSONC reader, which would be a forbidden edge) (no `serde`: AGENTS.md §3 rule 5
  approves `serde` only as postcard's derive companion). It names units explicitly in field names
  (`peak_power_w`, `freq_hz`, `rcs_m2`, `range_m`, `noise_figure_db`), so a scenario cannot be read
  in the wrong unit. A scenario is untrusted input in the sense that it is hand-written, so the
  decoder follows the operator's rule on decoders of untrusted input. It lands with phase 1a's first
  fields, not with phase 0 (section 9), and it binds:
  - **Caps, each a named constant with its reason, stated by phase 1a:** the file's size, checked
    before the file is read; the nesting depth, because a stack overflow in recursive descent
    cannot be caught; the length of a string and of a number's lexeme; the count of entries in
    every list; and a heightmap's width and height, whose product is taken by checked multiply and
    capped before anything is allocated.
  - **Rejections, each a typed error with an enumerated code:** an unknown field, as for playbooks;
    a **duplicate key**, because last-wins would be a silent fallback; a `/* */` block comment (only
    `//` line comments are accepted); a non-finite number (section 8.7); and a PGM heightmap whose
    `maxval` is outside 1 to 65 535 or whose sample count differs from width × height.
  - **A property test** that a crafted file only ever returns the typed error: a hand-rolled seeded
    generator (splitmix64, as `crates/sim/tests/pathing.rs` and `crates/mesher/tests/geometry.rs`
    already use), each failing case reproducible from its seed. `proptest` is not on the approved
    list and is not asked for.
  - **Lints:** the decoder module carries
    `#![deny(clippy::panic, clippy::arithmetic_side_effects, clippy::expect_used)]`, on top of the
    panic lints the wall never lifts (`indexing_slicing`, `unwrap_used`). A stricter module-level
    lint is not a contract change.
  - **Errors** are a typed enum (`thiserror` is not on the approved list, so a hand-written `Error`
    enum with `Display` and `std::error::Error`, which costs a few lines; section 8.7).
- **Out: tables and maps**, written as plain text: CSV for tables (range against SNR and Pd; SNR
  against Pd for each Swerling case; Doppler against velocity with blind speeds marked), and for a
  range-Doppler map a header line plus a row-major grid of dB values (CSV, or the portable greymap
  `PGM`, which needs no image crate and opens in any viewer). A golden test compares numbers against
  the published value with the source's tolerance; it never compares output files byte for byte.
- **Heightmaps:** a scenario may name a heightmap file (a grid of metres, CSV or PGM). Later, a
  `gamectl` subcommand or a test in `crates/gamectl/tests` may export the map generator's surface
  heights to that format, so the lab reads a game map without linking the sim. That exporter is a
  separate, later decision; it is not needed for phases 1 to 5.

### 8.4 Dependencies

The approved list (AGENTS.md §3 rule 5) has nothing for FFTs or special functions, so each is an
owner decision. Recommendation:

| Need | Option | Recommendation |
|---|---|---|
| FFT for range-Doppler and cross-ambiguity maps | `rustfft` 6.4.1 (2025-09-18; `MIT OR Apache-2.0`; runtime dependencies `num-complex`, `num-integer`, `num-traits`, `primal-check`, `strength_reduce`, `transpose`, all `MIT OR Apache-2.0`). Pure Rust, with SIMD code behind its default features `avx`, `sse` and `neon`. | **Write a small radix-2 FFT in the crate first** (about 80 lines: iterative Cooley–Tukey with bit reversal, power-of-two lengths only, zero-padding to the next power of two). It is easy to test exactly (case F-1): the DFT of an impulse, of a single complex tone, Parseval's identity, and agreement with a direct O(N²) DFT at N = 1 024 to 1e-12 relative. Ask for `rustfft` only if a phase measures the in-crate FFT as the bottleneck of a lab run that matters (a 2^24-point CAF batch, say). A lab is not a hot path. |
| Marcum Q, incomplete gamma, Bessel I₀ and I₁ | `statrs` (MIT), `puruspe` (`MIT OR Apache-2.0`), `special` (`Apache-2.0 OR MIT`). None implements the generalised Marcum Q, which is the function the lab most needs. | **Implement in the crate**, from the published algorithms (section 8.2), pinned by published tables. A dependency would supply the gamma functions but not the Marcum Q, and its accuracy over the far tails is not documented as tested against Pfa 1e-10; ours will be. |
| Complex numbers | `num-complex` | A 20-line `Complex64` struct in the crate. |
| Scenario parsing | `serde_json` | A strict hand-written JSON reader in the crate, a few hundred lines with its own property test (section 8.3). Not a dependency. |
| Complex least squares for ECA (phase 4b) | a linear-algebra crate (none researched) | **In the crate**: a Householder QR solve of the complex least-squares problem, better conditioned than the normal equations, tested against a constructed system with a known answer and against a Cholesky solve of the normal equations on well-conditioned systems. A singular or non-finite system is a typed error (section 8.7). |
| Seeded noise for synthetic signals and receiver noise (phase 4b) | `rand`, `rand_distr` (not on the approved list) | **In the crate**: a seeded generator (splitmix64, the mixer the workspace's tests already use) and Gaussian samples by Box–Muller, drawing its uniform from (0, 1] so that `ln 0` cannot occur. Every run records its seed and replays from it. |

So phases 1 to 6 need **no new dependency**, and the only candidate worth an owner ruling later is
`rustfft`. The workspace's supply-chain rules (cooldown, cargo-deny) apply to it unchanged if asked.

### 8.5 How the lab feeds the integer sim later

Item 138 (4) (d) sets the shape: in the sim, radar quantities are **integer log-domain values**
(centibels, cB: one hundredth of a bel, i.e. 0.1 dB), because `R⁴` and the spread of cross-sections
overflow Q16.16 and Q32.32. A port goes like this:

1. **The lab states the model in dB form**, the way the textbooks write the budget:
   `SNR_dB = P_t,dBW + G_t,dB + G_r,dB + 20 log10 λ + σ_dBsm − 30 log10 4π − 40 log10 R − 10 log10 (k T_s B) − L_dB`.
   Every term except `log10 R` is a constant per radar and target kind, so it becomes a rules-table
   row in cB, computed offline by the lab and checked by it. A new rules row is a `proto/**` change
   (AGENTS.md §12), so the port's pull request is a contract pull request. In the sim, radar joins
   item 138 (4) (c)'s sensor interface as one more sensor, beside spheres, unit vision, pickets,
   Survey and the Sensor Spire, and feeds the same two outputs.
2. **The sim evaluates `40 log10 R` from integers.** `R²` is already what the sim computes (`Sq`,
   Q32.32, squared distances: AGENTS.md §4.2), so `40 log10 R = 20 log10 R²`, taken from a lookup
   table of `log10` over the mantissa of `R²` plus `20 log10 2` (6.0206 dB) per bit of its
   exponent (found with `leading_zeros`). A 1 024-entry table over the mantissa errs by under
   0.005 dB if each entry is centred on its interval or the index rounds to nearest (0.0042 dB);
   truncating to the table's 10 bits with entries at the interval's start errs by up to 0.0085 dB
   (both computed here). It is determinism code (§5), generated once and committed, like the angle
   LUT.
3. **Thresholds are integers too.** The detection threshold for each (Pd, Pfa, Swerling case, n)
   the rules ask for is computed in the lab and stored in cB. The sim compares cB to cB; it never
   evaluates a probability. Where a random draw is wanted (a fluctuating target detected on some
   scans and not others), the sim draws from its own seeded stream against an integer probability
   the lab tabulated, and only if the owner wants chance in detection (an operator question,
   section 10).
4. **Doppler is radial velocity**, computed from per-tick position changes as item 138 (4) (d)
   says: `v_r = (Δp · r̂)`, compared against blind-speed bands and an MDV that the lab tabulates as
   integers in the game's speed unit (decivoxels per second, item 90).
5. **The differential test, without a sim edge.** The lab never depends on the sim, so the
   comparison runs through committed vectors, the workspace's rule for two implementations of one
   spec once they live in different crates.
   - **The file.** A grid of inputs in game units and, for each, the expected result **already
     converted to game units** (SNR in cB, detection range in voxels, radial speed in decivoxels per
     second) with a declared integer tolerance, for example 1 cB (0.1 dB), one voxel of range, one
     decivoxel per second. It lives outside `tests/golden`, at `tests/vectors/radar/<model>.json`,
     because it does not fit the golden step's `<area>/<case>/expected.<ext>` layout and golden
     formats are a §5 contract. The file and its format are a contract path of the port's pull
     request, named in it, since the sim's tests read them.
   - **The sim's side.** A test in the sim reads the file and asserts that its integer functions
     land inside the tolerance, with no float in the sim's test.
   - **The lab's side.** Regenerating the vectors is a **bless-only command** of the lab's binary,
     never a test. The lab's test recomputes each value in f64 and checks it against the committed
     integer within a stated margin: the committed integer must still be the value rounded to
     nearest, give or take 0.01 cB (0.001 dB) at a rounding boundary. It never compares bytes,
     because `log10`, `sin` and `exp` come from each platform's libm (MSVC's, glibc's and Apple's
     differ in the last bits), and a value near a rounding boundary could round differently on
     different operating systems. Re-blessing is a reviewed change, like any golden (AGENTS.md
     §5).
   - **Why a tolerance in game units.** The workspace's rule for two implementations of one spec
     asks for integers bit for bit and floats within a declared ULP tolerance. This port departs
     from it on purpose: the sim does not implement the lab's spec but a quantised approximation of
     it (cB, a lookup table, integer distances), so the declared tolerance is the approximation's
     error budget in game units, not a rounding allowance.

None of this needs a metric scale until step 2; the scale (metres per voxel) is item 138 (4) (e)'s
owner question and is listed again in section 10.

### 8.6 The passive receiver site (binding from phase 4a)

Decisions-log item 140 (2) puts secondary illuminators of opportunity on the roadmap, received
"by the same passive receiver that is used for catching solar ioo", with "different antennae
configurations but same relative location, height being variable by use of tower or tethered
drone", where a tethered balloon now stands for the drone (item 147). The roadmap phase is later, but the shape is binding now: phases 4a, 4b and 5 build the
passive receiver this way, so that adding an illuminator is a new scenario entry, not a rewrite.

- **One receiver site, with a geodetic anchor and a local frame.** The scenario gives the site's
  latitude, longitude and elevation, which SPA needs for the Sun's position
  (section 5.6), and the lab works in a local east-north-up frame centred on the site, in metres.
  Targets, terrestrial transmitters and the heightmap are placed in that frame.
- **A height that is a free parameter**, standing for a tower or a tethered balloon, measured above
  the heightmap's surface at the site. Line of sight over the heightmap (section 2.5), the radar
  horizon (`d = √(2 k_e R_e h + h²)`; for example 22.6 km at 30 m and 41.2 km at 100 m, computed
  here) and the flat-earth multipath factor all take it as an input, so raising the receiver
  changes all three at once.
- **One or more antenna configurations at that site**, each with its own pattern, gain, band and
  bandwidth: a Sun-tracking reference and a surveillance beam in phase 5, later an FM or DVB-T
  reference pointed at a transmitter in phase 6. All of them share **one phase centre**, at the
  site's position and height: a stated simplification of the owner's "same relative location",
  which leaves out the metres that separate real antennas on one mast. Each configuration feeds its
  own channel; a configuration names the band it receives, and a scenario that pairs an illuminator
  with a configuration outside its band is a typed decode error, not a silent zero.
- **A set of illuminators.** Phase 4b's generic synthetic illuminator first, then the Sun (phase 5:
  flux by band from section 5.2, position from section 5.6, unpolarised, at infinity), later
  terrestrial transmitters (phase 6: position, height, EIRP, centre frequency, bandwidth and signal
  model, from section 4.5's tables). The geometry code takes an illuminator at a finite distance or
  at infinity through one interface; the Sun is not a special case of the receiver, only of the
  illuminator.
- **No platform motion (ruled, item 140 (3) (d)).** The owner ruled: *"ignore drone wobble to
  simplify things - acknowledge this unrealism in comments"*. The receiver's position and height
  are fixed for a run, whether a tower or a tethered balloon sets the height. Platform motion, the
  position wander and the Doppler it puts on the reference channel, is not modelled, so the
  goldens of sections 7.6 and 7.8 apply unchanged. The ruling named the drone; it carries over to the tethered balloon that replaced it (item 147), whose sway in the wind it covers the same way.
- **Binding: the unrealism is stated in the code (the owner's ruling).** The code says plainly
  that a real tethered balloon sways and Doppler-shifts the reference channel, and that the lab
  ignores this by the owner's choice (decisions-log item 140 (3) (d)).
- **How that is enforced (engineering call; you can veto).** The comment is the doc of a
  `StaticReceiverSite` type whose fields are private. It is built once, from the decoded scenario,
  and the site's position, height and phase centre are read only through its methods, so every
  reader (the line-of-sight and horizon calls, the multipath factor, the bistatic geometry, the
  CAF's delay and Doppler reference) goes through the type that carries the comment, and the
  compiler, not a reviewer, keeps it that way.

### 8.7 Numeric safety (binding)

The wall lifts the cast lints (`as_conversions` and the `cast_*` family) for the whole crate, and
`f64 as usize` saturates and turns NaN into 0. NaN or infinity can come from `log10(0)`, from a
`P_fa` of 0 or less, from a range of 0, and from any division by a quantity that can be 0. Derived
counts feed allocations: a sample count is a rate times a time, and 2^24 complex samples are
256 MiB (computed here). So the lab binds its own rules, which hold in every phase:

- **Types that cannot hold bad values.** A `Probability` newtype holds a value in the open interval
  (0, 1); `Metres`, `Hertz`, `Seconds`, `Watts` and the like hold positive finite values; a
  constructor that is handed anything else returns a typed error. The decoder builds these types,
  so a model function never sees a raw scenario number.
- **Non-finite values are errors.** The decoder rejects non-finite numbers, and any non-finite
  intermediate a model produces is a typed error at the point it appears, never a NaN carried into
  a table.
- **One checked float-to-integer helper.** It takes a finite value and a target range and returns
  the integer or a typed error; `as` is never applied to a float anywhere in the crate. Rounding is
  named in the helper's call (to nearest, down or up), never implied.
- **Every derived count is capped before it is allocated.** Sample counts, FFT lengths, grid sizes
  and heightmap cells are computed with checked arithmetic, through the helper above, and compared
  with a named cap stated by the phase that introduces it (2^24 complex samples a channel is the
  example above), before any buffer is allocated. Exceeding a cap is a typed error, never a
  truncation.
- **Errors are enumerated.** The lab's error enum carries an enumerated code per failure,
  implements `Display` and `std::error::Error` (as `crates/gateway/src/error.rs` and
  `crates/mesher/src/lib.rs` do), and no predictable failure panics, unwraps or falls back
  silently.

## 9. Phased build plan

Each phase is one pull request of one concern, opens and stops like any lane, and lands its golden
cases from section 7 with it. A phase is done when every case it names passes inside the source's
tolerance. Nothing in phases 0 to 6 touches the sim, the gateway or the client; phase 7's ports do
touch the sim, as its row says.

| Phase | Scope | Golden cases | Contract paths and named places |
|---|---|---|---|
| **0. The crate** (approved in principle, item 140 (3) (a)) | Contract changes only: the empty walled crate, `publish = false`; its `WALLED_PACKAGES` entry; its `CLIENT_WALL` line, with the list's doc and AGENTS.md §4.9's bullet generalised to walled crates that must not reach the core; the lab's `Cargo.lock` test that it has no `pharmakos-*` dependency; and `wall-guard`'s new relation that no package root reaches a never-shipped walled crate, with its test (section 8.1). No decoder: it lands with phase 1a's first fields. | The lab's `Cargo.lock` test; the new relation's refusal test; `cargo xtask ci` green. | Contract paths: `xtask/src/main.rs` (`WALLED_PACKAGES`, `CLIENT_WALL` and its doc comment, the new relation and its test, and any comment that counts the walled crates or the never-shipped ones); `clippy.toml`'s header; AGENTS.md §3's crate map and §4.9 (the walled list and the `CLIENT_WALL` bullet); the root `Cargo.toml`'s members and `Cargo.lock`. More if phase 0 keeps the research-guard join (section 8.1). Named places: `xtask`; `crates/radar-lab`. The pull request opens and stops (item 140 (3) (a)). |
| **1a. The range equation** | The scenario decoder with its caps, rejections, seeded property test and module lints (section 8.3), for this phase's fields; the numeric types and checked helper of section 8.7; dB helpers; the range equation (power and energy forms) with losses and system noise temperature; RCS of canonical shapes. | R-1 to R-6 (R-5 and R-6 take their detection values as literal inputs); the decoder's property test; dB round trips. | None beyond phase 0. |
| **1b. Special functions** | The regularised incomplete gamma `P` and `Q`, each tail computed directly; the generalised Marcum Q by Gil, Segura and Temme's algorithm (or Shnidman's 1989 method); Bessel `I₀` and `I₁` if the algorithm needs them. | The identities of section 7.2 (`Q₁(0, b) = e^{−b²/2}`, monotonicity, the two tails summing to 1 to 1e-12 down to `P_fa = 1e-12`), and `Q(n, x)` against its finite sum for integer `n`. | None. |
| **1c. Detection** | Swerling 0 to 4 for one pulse and for `n` pulses (exact, through 1b); Albersheim; Shnidman; coherent detection of a complex signal and the real-signal threshold; coherent and noncoherent integration gain and loss. | D-1 to D-11. | None. |
| **2. Line of sight, terrain and clutter** | Ray march over a heightmap (Amanatides–Woo traversal), stated in its code as the 2.5D simplification of section 2.5; 4/3-earth horizon; the optional flat-earth two-ray propagation factor (off by default); constant-gamma land clutter; pulse- and beam-limited clutter cell; signal-to-clutter ratio. | L-1 to L-3, C-1, C-2, and constructed masking cases. | None. |
| **3. Doppler and MTI** | `Complex64`; the in-crate FFT; Doppler shift, PRF regimes, blind speeds and staggered PRF, single and double delay-line cancellers, improvement factor, MDV; range-Doppler map of an LFM pulse train (matched filter, slow-time FFT). | M-1 to M-3, F-1. | None. |
| **4a. The receiver site and bistatic geometry** | The `StaticReceiverSite` of section 8.6 (geodetic anchor, local east-north-up frame, height above the heightmap's surface, one shared phase centre, antenna configurations, an illuminator set); bistatic geometry (range sum, bistatic angle, ovals of Cassini), the bistatic radar equation, bistatic Doppler. | B-1 to B-4, P-1 to P-4 as bare link budgets: fixtures that validate the chain, not illuminator entries. | None. |
| **4b. Cross-ambiguity and cancellation** | One generic synthetic illuminator (band-limited noise); the seeded generator and Gaussian sampling of section 8.4; the batched cross-ambiguity function; direct-path cancellation by ECA, with the in-crate complex least-squares solver. Terrestrial illuminator entries are phase 6's. | P-2's processing gain recovered by the CAF on the synthetic signal; constructed CAF and ECA cases with known answers (a target at a known delay and Doppler lands in its cell; ECA's residual against a known direct-path level), stated as constructed. The long runs are `#[ignore]`d hand-run checks (section 8.1). | None. |
| **5. The Sun** | Solar ephemeris (NREL SPA, with the Astronomical Almanac's low-precision formula as a cheaper second implementation checked against it); solar flux by band and its variability as a scenario input; the Sun as an illuminator, with a Sun-tracking reference and a surveillance beam at the site; the noise-illuminator correlation SNR; the vehicle-target integration-time table of section 5.5. | S-1 to S-3, N-1, N-2; section 5.5's table as a simulation target, not a golden. | None. |
| **6. Secondary illuminators of opportunity (roadmap, not committed scope)** | Decisions-log item 140 (2): FM, DVB-T and the like, received at the same site by the same receiver as the Sun. Terrestrial illuminator entries (section 4.5's tables: EIRP, position, height) join the Sun in the scenario's illuminator set, with the antenna configurations that receive them (an FM or DVB-T reference pointed at its transmitter); the CAF, ECA and site are phase 4's. | B-2 to B-4, P-1 to P-4 (the passive bistatic cases), run through the receiver site with their transmitters as illuminator entries; L-1 to L-3 for the height. | None. |
| **7. Experiments** | The owner's "later we will try making improvements and see how the modifications work in the sim": modifications are tried in the lab against the phase 1 to 6 baselines, and a modification that graduates is ported by the steps of section 8.5. Which stage holds the in-sim layer is an operator question (section 10, question 12). | Each experiment states its own baseline. | Any port is sim determinism code (§5), a `proto/**` change for its rules rows (AGENTS.md §12), and the vectors file of section 8.5. |

After phase 0: phase 1b needs nothing but phase 0; phase 1a needs phase 0; phase 1c needs 1a and
1b; phase 2 needs 1a; phase 3 needs 1c's detection functions; phase 4a needs phase 2 (line of
sight, the horizon and multipath take the site's height) and phase 1a; phase 4b needs phase 3's FFT
and phase 4a's site; phase 5 needs phase 4b's cross-ambiguity function and phase 4a's site; phase 6
needs phase 5's site with the Sun in it. AGENTS.md §6's one-crate-one-agent rule means the lab's
phases run **one at a time** in its own crate, in the order 0, 1a, 1b, 1c, 2, 3, 4a, 4b, 5, 6,
within §6's cap of two to three lanes. The lab starts **after S2** (item 140 (3) (c)) and runs
**beside S3's lanes**, not in a gap between S2 and S3 (item 141 (2)): neither S3 nor the fun gate
waits for it. S2's merge delegation (item 141 (1)) does not cover the lab, so its pull requests
are the owner's to merge unless the owner extends the delegation at S2's stage demo.

## 10. Open questions

Each is classed as **engineering: decide** (with the call this memo makes, which the owner may
veto) or **operator: intent, priority, risk or policy** (the owner's). The line between them, where
it is close: a choice that decides whether the lab can represent something the owner named goes to
the owner (the tethered balloon's motion, question 9); a default for a model that stays available and
leaves every published golden unchanged is engineering (multipath off by default, engineering
call 8).

### Operator: intent, priority, risk or policy

The owner ruled on questions 1, 2 and 4 on 2026-10-14 ("approve the crate, never ship, after S2";
item 140 (3) (a) to (c)), on question 9 the same day (item 140 (3) (d)), and on where the lab runs
against S3 the same day ("beside S3, renew the delegation for S2"; item 141 (2)). The rest stay
open.

1. **Ruled 2026-10-14 (item 140 (3) (b)): never ships.** Item 138 (4) (b) had left it open. The
   lab is like `bench`: `publish = false`, outside `cargo xtask package`'s zips, and no shipped crate
   depends on it, so it adds nothing
   to the game's licence, size or support surface, and a later decision to ship a radar view is a
   separate change. *Downside:* players and modders see no lab unless they build the repository.
2. **Ruled 2026-10-14 (item 140 (3) (a)): the crate is approved in principle.** The owner approved
   the crate and the contract changes section 8.1 names; their exact shape is left to phase 0.
   AGENTS.md §5 approves an actual diff, so phase 0's pull request still names every contract path
   it touches, opens and stops. Since the ruling, section 8.1 has dropped the research-guard join
   and added the never-shipped check; both are engineering calls inside what the owner approved,
   open to the owner's overrule.
3. **Dependencies.** Phases 1 to 6 need none (section 8.4). The one candidate is `rustfft` 6.4.1
   (`MIT OR Apache-2.0`, six runtime dependencies, all `MIT OR Apache-2.0`), and only if the in-crate
   FFT is measured as a bottleneck. *Recommendation:* do not approve anything now.
4. **Ruled 2026-10-14 (item 140 (3) (c) and item 141 (2)): after S2, beside S3.** Item 138 (4)
   recorded path 3's downside: the lab pulls effort from the game, and the fun gate and v1 may
   slip. The owner ruled that S2 comes first and the lab's phases follow, and then that they run
   beside S3's lanes rather than in a gap between S2 and S3. The trade-off the owner ruled on named
   the fun gate, so neither S3 nor the fun gate waits for the lab. *Downside:* lab lanes share
   AGENTS.md §6's cap and the reviews with S3, and its pull requests wait for the owner's merge
   unless the owner extends the merge delegation at S2's stage demo.
5. **The game's metric scale** (metres per voxel), item 138 (4) (e). It decides whether Earth
   curvature matters (section 2.5), whether walkers clear a ground radar's MDV (section 3.1), and
   whether solar detection of a vehicle takes seconds or hours (section 5.5). The lab needs no
   answer in phases 1 to 5, since its scenarios are in metres; a port to the sim does.
6. **The Pall lore.** Does a sun stand above the Pall as a radio illuminator? Does a match have a
   sun's path through its day (a new rule, and a new input to the sim)? How much does ash attenuate
   and clutter in the chosen band (section 6)? These are lore and rules questions for the owner.
7. **Realism against fiction.** If the lab shows that a physically real solar passive radar finds
   nothing on a playable map, does the game keep it real (and invisible), or bend it? Section 5.5
   suggests the answer may be "it finds things at short range, slowly", which changes the question
   rather than settling it.
8. **Chance in detection.** A real detector is probabilistic. Does the in-sim radar, when ported,
   detect by a seeded draw against a tabulated `P_d` (chance on screen, still deterministic), or by
   a threshold on SNR (no chance)? This is a design-intent question with a determinism cost either
   way: a draw needs a new RNG stream (§4.7, additive).
9. **Ruled 2026-10-14 (item 140 (3) (d)): no platform motion.** "ignore drone wobble to simplify
   things - acknowledge this unrealism in comments". The receiver's height is fixed for a run,
   tower or tethered balloon; the balloon's sway and the Doppler it puts on the reference
   channel are not modelled, and the code says so wherever the receiver's height or position is
   set or read (section 8.6). The ruling named the drone and carries over to the tethered balloon (item 147).
10. **When the secondary-illuminator phase runs.** Item 140 (2) is "future future" roadmap; phase 6
    is listed but not committed. Whether it runs straight after phase 5 or waits for a later ruling
    is the owner's.
11. **Which illuminators exist in the fiction.** FM, television and cellular towers are the real
    world's illuminators of opportunity, and item 140 (2) puts them on the lab's roadmap; whether any
    stand in Pharmakos' world, or only the Sun and the seats' own transmitters, is lore.
12. **Which stage gets in-sim radar.** Item 140 (1) reads the owner's words as path 3: the lab
    first, and the in-sim layer ("see how the modifications work in the sim") later. No stage holds
    the in-sim layer, and no plan schedules it. Whether it joins a stage before v1 (with item 138
    (4) (c)'s sensor interface and S4's Sensor Spire, say) or waits for after v1 is the owner's;
    the lab needs no answer until phase 7 ports something.

### Engineering: decide

1. **f64 everywhere, no extended precision** (section 8.2). Decided; you can veto.
2. **No sim edge.** The lab depends on no `pharmakos-*` crate: a `CLIENT_WALL` line, with the
   list's doc and AGENTS.md §4.9 generalised to walled crates that must not reach the core, plus a
   lab test over `Cargo.lock`; no `GUARDED_PACKAGES` join; and a new `wall-guard` relation that no
   package root reaches a never-shipped walled crate (section 8.1). It feeds the sim through
   committed vectors in game units, regenerated only by a bless command and checked in f64 against
   the committed integers within a stated margin, never byte for byte (section 8.5). Decided; you
   can veto.
3. **Not in the cross-OS guard; goldens against published values with stated tolerances**, run in
   the ordinary `test` step on all three operating systems; the heavy simulations are `#[ignore]`d
   checks run by hand (section 8.1). Decided; you can veto.
4. **In-crate FFT, special functions, complex least-squares solver and seeded generator** before
   any dependency, each pinned by identities and published or constructed values (section 8.4).
   Decided; you can veto.
5. **Each detection function tested against its own published values**, never against another's
   (section 2.3). Decided.
6. **The MathWorks passive example's golden is the corrected K** (225.54 dB), with the published
   230.5413 dB recorded beside it (section 4.6). Decided.
7. **Only V, V-calc and X values become goldens**; S and R values wait in section 7.9, as do the
   detectability page's Barton values until the lab has a Barton function. Every X value is
   printed by appendix A's reference script (section 7.10). Decided.
8. **Multipath as an optional flat-earth factor, off by default**; no diffraction or curved-earth
   propagation until an experiment needs it (section 2.5). Terrain masking over a heightmap is a
   stated 2.5D simplification, and a 3D voxel traversal comes before any masking port. Decided;
   you can veto.
9. **SPA as the reference ephemeris, the Almanac's low-precision formula as the second
   implementation** (section 5.6). Decided; you can veto.
10. **Scenario format: strict JSON with `//` comments, units in field names, capped and typed
    decode with duplicate keys rejected and a seeded property test; tables as CSV, maps as CSV or
    PGM** (section 8.3). Decided; you can veto.
11. **No `serde`, no `thiserror` and no `proptest`** in the lab: none is on AGENTS.md §3's approved
    list for this use, and a hand-written decoder, error enum and seeded generator cost little.
    Decided; you can veto (asking for them would be an owner dependency decision).
12. **Phase order** as section 9, with phase 0 contract-only and phases 1 and 4 split. Decided; you
    can veto.
13. **One receiver site from phase 4a** (section 8.6): a geodetic anchor and a local frame, a
    height above the surface, antenna configurations at one shared phase centre and an illuminator
    set, so phase 6 adds illuminators without a rewrite. Decided on item 140 (2); you can veto the
    details.
14. **Numeric safety** (section 8.7): newtypes that cannot hold bad values, non-finite values as
    typed errors, one checked float-to-integer helper, derived counts capped before allocation, and
    an enumerated error enum that implements `std::error::Error`. Decided; you can veto.
15. **The no-motion ruling enforced by a type**: `StaticReceiverSite`, with private fields and the
    unrealism comment as its doc (section 8.6). Decided; you can veto.


## 11. References

Books

- Barton, D. K. *Radar Equations for Modern Radar*. Artech House, 2013. ISBN 978-1-60807-521-8.
- Billingsley, J. B. *Low-Angle Radar Land Clutter: Measurements and Empirical Models*. SciTech/
  William Andrew, 2002. ISBN 978-1-891121-16-6.
- Blake, L. V. *Radar Range-Performance Analysis*. Artech House, 1986. ISBN 0-89006-224-2.
- Griffiths, H. D. and Baker, C. J. *An Introduction to Passive Radar*. Artech House, 1st ed. 2017,
  ISBN 978-1-63081-036-8; 2nd ed. 2022, ISBN 978-1-63081-840-1.
- Knott, E. F., Shaeffer, J. F. and Tuley, M. T. *Radar Cross Section*, 2nd ed. Artech House, 1993
  (R; not read in this research).
- Levanon, N. and Mozeson, E. *Radar Signals*. Wiley, 2004 (ISBN not verified).
- Mahafza, B. R. *Radar Systems Analysis and Design Using MATLAB*, 3rd ed. CRC Press, 2013.
  ISBN 978-1-4398-8495-9.
- Meeus, J. *Astronomical Algorithms*, 2nd ed. Willmann-Bell, 1998. ISBN 0-943396-61-1 (R).
- Nathanson, F. E., Reilly, J. P. and Cohen, M. N. *Radar Design Principles*, 2nd ed. McGraw-Hill,
  1991. ISBN 0-07-046052-3.
- Peebles, P. Z. *Radar Principles*. Wiley, 1998. ISBN 0-471-25205-0.
- Richards, M. A. *Fundamentals of Radar Signal Processing*. McGraw-Hill: 1st ed. 2005, ISBN
  0-07-144474-2; 2nd ed. 2014, ISBN 978-0-07-179832-7; 3rd ed. 2022, ISBN 978-1-260-46871-7.
- Richards, M. A., Scheer, J. A. and Holm, W. A. (eds). *Principles of Modern Radar: Basic
  Principles*. SciTech, 2010. ISBN 978-1-891121-52-4.
- Skolnik, M. I. *Introduction to Radar Systems*, 3rd ed. McGraw-Hill, 2001. ISBN 0-07-288138-0
  (international edition 0-07-290980-3).
- Skolnik, M. I. (ed.). *Radar Handbook*, 3rd ed. McGraw-Hill, 2008. ISBN 978-0-07-148547-0.
- Willis, N. J. *Bistatic Radar*, 2nd ed. SciTech, 2005. ISBN 978-1-891121-45-6;
  DOI 10.1049/SBRA003E (Crossref dates the DOI's record 2004).
- Willis, N. J. and Griffiths, H. D. (eds). *Advances in Bistatic Radar*. SciTech, 2007.
  ISBN 978-1-891121-48-7 (S).

Detection theory

- Albersheim, W. J. "A closed-form approximation to Robertson's detection characteristics." *Proc.
  IEEE* 69(7):839, 1981. DOI 10.1109/PROC.1981.12082.
- Gil, A., Segura, J. and Temme, N. M. "Computation of the Marcum Q-function." *ACM Trans. Math.
  Softw.* 40(3), 2014. DOI 10.1145/2591004; arXiv 1311.0681.
- Helstrom, C. W. "Computing the generalized Marcum Q-function." *IEEE Trans. Inf. Theory*
  38(4):1422–1428, 1992. DOI 10.1109/18.144731.
- Marcum, J. I. "A statistical theory of target detection by pulsed radar." *IRE Trans. Inf.
  Theory* IT-6(2):59–267, 1960. DOI 10.1109/TIT.1960.1057560.
- Robertson, G. H. "Operating characteristics for a linear detector of CW signals in narrow-band
  Gaussian noise." *Bell Syst. Tech. J.* 46(4):755–774, 1967.
  DOI 10.1002/j.1538-7305.1967.tb01072.x.
- Shnidman, D. A. "The calculation of the probability of detection and the generalized Marcum
  Q-function." *IEEE Trans. Inf. Theory* 35(2):389–400, 1989. DOI 10.1109/18.32133.
- Shnidman, D. A. "Determination of required SNR values." *IEEE Trans. Aerosp. Electron. Syst.*
  38(3):1059–1064, 2002. DOI 10.1109/TAES.2002.1039422.
- Swerling, P. "Probability of detection for fluctuating targets." *IRE Trans. Inf. Theory*
  IT-6(2):269–308, 1960. DOI 10.1109/TIT.1960.1057561.

Reports

- Blake, L. V. *A Guide to Basic Pulse-Radar Maximum-Range Calculation, Part 1*. NRL Report 6930,
  1969. DTIC AD0701321. Part 2: NRL Report 7010, DTIC AD0703211. FORTRAN version: NRL Report 7448,
  DTIC AD0749686.
- Griffiths, H. D. "Passive bistatic radar." NATO RTO-EN-SET-119(2010), paper 3, 2010.
- O'Hagan, D. W., Baker, C. J. and Griffiths, H. D. "Signal and interference analysis for a passive
  bistatic radar." NATO RTO-MP-SET-095, paper 21, 2006 (title as recorded by the research).
- Reda, I. and Andreas, A. *Solar Position Algorithm for Solar Radiation Applications*. NREL
  Technical Report NREL/TP-560-34302 (revised 2008).
- Hirsch, M. and Kidd, R. *Passive FM radar* report, 2013. scivision.dev/pdf/passivefmradar-report.pdf.

Passive and bistatic radar

- Colone, F., O'Hagan, D. W., Lombardo, P. and Baker, C. J. "A multistage processing algorithm for
  disturbance removal and target detection in passive bistatic radar." *IEEE Trans. Aerosp.
  Electron. Syst.* 45(2):698–722, 2009. DOI 10.1109/TAES.2009.5089551.
- Colone, F. et al. "WiFi-based passive bistatic radar." *IEEE Trans. Aerosp. Electron. Syst.*
  48(2):1061–1079, 2012. DOI 10.1109/TAES.2012.6178049.
- Griffiths, H. D. and Baker, C. J. "Passive coherent location radar systems. Part 1: performance
  prediction." *IEE Proc. Radar Sonar Navig.* 152(3):153–159, 2005. DOI 10.1049/ip-rsn:20045082.
- Howland, P. E., Maksimiuk, D. and Reitsma, G. "FM radio based bistatic radar." *IEE Proc. Radar
  Sonar Navig.* 152(3):107–115, 2005. DOI 10.1049/ip-rsn:20045077.
- Kulpa, K. S. and Czekała, Z. "Masking effect and its removal in PCL radar." *IEE Proc. Radar Sonar
  Navig.* 152(3):174–178, 2005. DOI 10.1049/ip-rsn:20045026.
- Kuschel, H., Cristallini, D. and Olsen, K. E. "Tutorial: Passive radar tutorial." *IEEE Aerosp.
  Electron. Syst. Mag.* 34(2):2–19, 2019. DOI 10.1109/MAES.2018.160146.
- Malanowski, M. et al. IET International Conference on Radar Systems, 2012.
  DOI 10.1049/cp.2012.1578.
- Palmer, J. E. et al. "DVB-T passive radar signal processing." *IEEE Trans. Signal Process.*
  61(8):2116–2126, 2013. DOI 10.1109/TSP.2012.2236324.
- Pető, T. and Seller, R. *Archives of Electrical Engineering* 66(1):5–16, 2017.
  DOI 10.1515/aee-2017-0001.
- Pető, T. and Seller, R. IntechOpen chapter, 2018. DOI 10.5772/intechopen.71289.

Solar and planetary passive sounding, and the Sun

- Gary, D. E. arXiv 1901.09262, 2019 (the 2006-12-06 flare's flux).
- Kansabanik, D. Thesis, arXiv 2310.16072, 2023 (quiet-Sun flux at low frequency).
- Michalsky, J. J. "The Astronomical Almanac's algorithm for approximate solar position
  (1950–2050)." *Solar Energy* 40(3):227–235, 1988. DOI 10.1016/0038-092X(88)90045-X. Errata:
  41(1):113, DOI 10.1016/0038-092X(88)90122-3; 43(5):323, DOI 10.1016/0038-092X(89)90122-9.
  Spencer's comment: 42(4):353, DOI 10.1016/0038-092X(89)90039-X.
- Peters, S. T. et al. *IEEE Trans. Geosci. Remote Sens.* 56(12):7338–7349, 2018.
  DOI 10.1109/TGRS.2018.2850662.
- Peters, S. T. et al. *Geophys. Res. Lett.* 48(14), e2021GL092450, 2021.
  DOI 10.1029/2021GL092450.
- Peters, S. T. et al. "Passive synthetic aperture radar imaging using radio-astronomical sources."
  *IEEE Trans. Geosci. Remote Sens.* 59(11):9144–9159, 2021. DOI 10.1109/TGRS.2021.3050429.
- Peters, S. T. et al. IGARSS 2019. DOI 10.1109/IGARSS.2019.8897880.
- "Spatial Coherence Constraints on Passive Radar Sounding With Radio-Astronomical Sources." *IEEE
  Trans. Geosci. Remote Sens.* 62 (2024) 4302612. DOI 10.1109/TGRS.2024.3456049 (S; title as the
  numbers review gives it).
- Reda, I. and Andreas, A. "Solar position algorithm for solar radiation applications." *Solar
  Energy* 76(5):577–589, 2004. DOI 10.1016/j.solener.2003.12.003. Corrigendum 81(6):838, 2007.
- Reid, H. A. S. and Ratcliffe, H. arXiv 1404.6117, 2014 (type III bursts).
- Romero-Wolf, A. et al. "A passive probe for subsurface oceans and liquid water in Jupiter's icy
  moons." *Icarus* 248:463–477, 2015. DOI 10.1016/j.icarus.2014.10.043; arXiv 1404.1876.
- Romero-Wolf, A. et al. arXiv 2305.05382, 2023.
- Schroeder, D. M. et al. *Planet. Space Sci.* 134:52–60, 2016. DOI 10.1016/j.pss.2016.10.007.
- Spencer, J. W. "Fourier series representation of the position of the sun." *Search* 2(5):172,
  1971.
- Tapping, K. F. "The 10.7 cm solar radio flux (F10.7)." *Space Weather* 11:394–406, 2013.
  DOI 10.1002/swe.20064 (S).

Clutter

- Barton, D. K. "Land clutter models for radar design and analysis." *Proc. IEEE* 73(2):198–204,
  1985. DOI 10.1109/PROC.1985.13133. The primary source for the Barton `γ` values of section 2.6,
  not read in this research (section 7.9).

Ash

- Marzano, F. S., Vulpiani, G. and Rose, W. I. "Microphysical characterization of microwave radar
  reflectivity due to volcanic ash clouds." *IEEE Trans. Geosci. Remote Sens.* 44(2):313–327, 2006.
  DOI 10.1109/TGRS.2005.861010 (S).
- Marzano, F. S., Barbieri, S., Vulpiani, G. and Rose, W. I. "Volcanic ash cloud retrieval by
  ground-based microwave weather radar." *IEEE Trans. Geosci. Remote Sens.* 44(11):3235–3246, 2006.
  DOI 10.1109/TGRS.2006.879116 (S).


Algorithms and online references

- Amanatides, J. and Woo, A. "A fast voxel traversal algorithm for ray tracing." *Eurographics '87*,
  1987, pp. 3–10 (R).
- MathWorks documentation (Radar Toolbox and Phased Array System Toolbox reference pages and
  examples): `radareqsnr`, `radareqrng`, `albersheim`, `shnidman`, `rocsnr`, `npwgnthresh`,
  `horizonrange`, `constantGammaClutter`, `surfacegamma`, `landreflectivity`, `speed2dop`,
  `ambgfun`, "Modeling radar detectability factors", "Designing a basic monostatic pulse radar",
  "Passive Bistatic Radar Performance Assessment", the staggered-PRF MTI example. mathworks.com/help.
  Read in October 2026; vendor documentation, not peer-reviewed, used because every input is
  printed.
- NOAA Global Monitoring Laboratory, solar calculation details: gml.noaa.gov/grad/solcalc/calcdetails.html.
- NOAA Space Weather Prediction Center, solar radio flux: swpc.noaa.gov (the `solar-radio-flux.json`
  product).
- US Naval Observatory, "Approximate solar coordinates": aa.usno.navy.mil/faq/sun_approx.

## Appendix A. The reference script

Section 7.10 says what this is and how to run it. It is documentation: copy it to a file outside the
repository before running it.

```python
"""Reference arithmetic for docs/design/radar-lab-research.md.

Documentation, not game code: no build step, test or CI job runs it, and
nothing ships it. It reproduces every value the memo flags X (an exact or
formula value with no published output to compare against, confirmed by two
independent implementations) and the memo's own computed figures, so that a
reader can check them without the research's working.

Standard library only. Run it isolated:

    python -I radar-lab-reference.py   # copied out of the memo, outside the repository

The exact detection values are computed twice, by independent methods (a
series or closed form, and numerical quadrature of the density), and the
script fails if the two disagree by more than 1e-4 dB. The other
implementation of every X value is the memo's numbers review (2026-10-14),
which recomputed them independently.
"""

import math
import sys

C = 299_792_458.0  # m/s
K_B = 1.380649e-23  # J/K, exact since the 2019 SI redefinition
T0 = 290.0  # K


def db(x):
    return 10.0 * math.log10(x)


def undb(x_db):
    return 10.0 ** (x_db / 10.0)


# --- Regularised incomplete gamma, two ways ---------------------------------


def gamma_p_series(a, x):
    """Lower regularised P(a, x) by its power series (x < a + 1)."""
    if x <= 0.0:
        return 0.0
    term = 1.0 / a
    total = term
    n = a
    for _ in range(100_000):
        n += 1.0
        term *= x / n
        total += term
        if abs(term) < abs(total) * 1e-17:
            break
    return total * math.exp(-x + a * math.log(x) - math.lgamma(a))


def gamma_q_cf(a, x):
    """Upper regularised Q(a, x) by Lentz's continued fraction (x >= a + 1)."""
    tiny = 1e-300
    b = x + 1.0 - a
    c = 1.0 / tiny
    d = 1.0 / b
    h = d
    for i in range(1, 100_000):
        an = -i * (i - a)
        b += 2.0
        d = an * d + b
        if abs(d) < tiny:
            d = tiny
        c = b + an / c
        if abs(c) < tiny:
            c = tiny
        d = 1.0 / d
        delta = d * c
        h *= delta
        if abs(delta - 1.0) < 1e-16:
            break
    return math.exp(-x + a * math.log(x) - math.lgamma(a)) * h


def gamma_q(a, x):
    """Upper regularised Q(a, x), each tail computed directly."""
    if x <= 0.0:
        return 1.0
    if x < a + 1.0:
        return 1.0 - gamma_p_series(a, x)
    return gamma_q_cf(a, x)


def gamma_p(a, x):
    if x <= 0.0:
        return 0.0
    if x < a + 1.0:
        return gamma_p_series(a, x)
    return 1.0 - gamma_q_cf(a, x)


def gamma_q_int_sum(n, x):
    """Q(n, x) for integer n by its finite sum: an independent check."""
    term = 1.0
    total = 1.0
    for k in range(1, n):
        term *= x / k
        total += term
    return math.exp(-x) * total


def bisect(f, lo, hi, iterations=200):
    """Root of an increasing f on [lo, hi]."""
    flo = f(lo)
    for _ in range(iterations):
        mid = 0.5 * (lo + hi)
        fmid = f(mid)
        if (fmid > 0.0) == (flo > 0.0):
            lo, flo = mid, fmid
        else:
            hi = mid
    return 0.5 * (lo + hi)


def threshold(n, pfa):
    """T with Q(n, T) = pfa: the square-law threshold for n pulses."""
    t_a = bisect(lambda t: math.log(pfa) - math.log(gamma_q(n, t)), 0.0, 400.0, 80)
    t_b = bisect(lambda t: math.log(pfa) - math.log(gamma_q_int_sum(n, t)), 0.0, 400.0, 80)
    if abs(t_a - t_b) > 1e-9 * t_a:
        sys.exit(f"threshold disagrees: {t_a} against {t_b}")
    return t_a


# --- Square-law detection: method A (series and closed forms) ---------------


def pd_sw0_series(n, snr, t):
    """Steady target: Poisson mixture of Q(n + k, T), lambda = n * snr."""
    lam = n * snr
    if lam == 0.0:
        return gamma_q(n, t)
    spread = 12.0 * math.sqrt(lam) + 20.0
    k0 = max(0, int(lam - spread))
    k1 = int(lam + spread)
    q = gamma_q(n + k0, t)
    log_w = -lam + k0 * math.log(lam) - math.lgamma(k0 + 1)
    total = 0.0
    log_inc = (n + k0) * math.log(t) - t - math.lgamma(n + k0 + 1)
    for k in range(k0, k1 + 1):
        total += math.exp(log_w) * q
        # Q(a + 1, t) = Q(a, t) + t^a e^-t / Gamma(a + 1)
        q += math.exp(log_inc)
        log_inc += math.log(t) - math.log(n + k + 1)
        log_w += math.log(lam) - math.log(k + 1)
    return min(total, 1.0)


def pd_sw1_closed(n, snr, t):
    """Swerling 1 (scan to scan), n pulses: Swerling's closed form."""
    if n == 1:
        return math.exp(-t / (1.0 + snr))
    ratio = 1.0 + 1.0 / (n * snr)
    return (
        1.0
        - gamma_p(n - 1, t)
        + ratio ** (n - 1) * gamma_p(n - 1, t / ratio) * math.exp(-t / (1.0 + n * snr))
    )


def pd_sw2_closed(n, snr, t):
    """Swerling 2 (pulse to pulse), n pulses: Q(n, T / (1 + snr))."""
    return gamma_q(n, t / (1.0 + snr))


# --- Square-law detection: method B (quadrature of the density) -------------


def simpson(f, a, b, intervals):
    h = (b - a) / intervals
    total = f(a) + f(b)
    for i in range(1, intervals):
        total += (4.0 if i % 2 else 2.0) * f(a + i * h)
    return total * h / 3.0


def log_bessel_i(nu, x):
    """log I_nu(x) by its power series, each term from the last."""
    if x == 0.0:
        return 0.0 if nu == 0 else -math.inf
    q = 0.25 * x * x
    term = 1.0
    total = 1.0
    k = 0
    while True:
        k += 1
        term *= q / (k * (k + nu))
        total += term
        if k > x and term < total * 1e-17:
            break
    return nu * math.log(0.5 * x) - math.lgamma(nu + 1) + math.log(total)


def pd_sw0_quad(n, snr, t):
    """Steady target: integrate the noncentral density of sum |x|^2 above T."""
    lam = n * snr

    def density(y):
        if y <= 0.0:
            return 0.0
        lg = -(y + lam) + 0.5 * (n - 1) * (math.log(y) - math.log(lam))
        return math.exp(lg + log_bessel_i(n - 1, 2.0 * math.sqrt(lam * y)))

    top = n + lam + 15.0 * math.sqrt(n + 2.0 * lam)
    return simpson(density, t, top, 2000)


def pd_sw1_quad(n, snr, t):
    """Swerling 1: average the steady-target Pd over an exponential mean SNR.

    The integrand is integrated on [0, 3]. The steady-target Pd rises with u,
    so the tail beyond 3 lies between Pd(3) e^-3 and e^-3; the midpoint is
    taken, and the script stops if that bound exceeds 1e-12.
    """
    edge = pd_sw0_series(n, snr * 3.0, t)
    if (1.0 - edge) * math.exp(-3.0) > 1e-12:
        sys.exit(f"Swerling 1 quadrature: Pd at u = 3 is only {edge}")
    body = simpson(lambda u: pd_sw0_series(n, snr * u, t) * math.exp(-u), 0.0, 3.0, 3000)
    return body + 0.5 * (1.0 + edge) * math.exp(-3.0)


def pd_sw2_quad(n, snr, t):
    """Swerling 2: integrate the gamma density of sum |x|^2 above T."""
    m = 1.0 + snr

    def density(y):
        return math.exp((n - 1) * math.log(y) - y / m - n * math.log(m) - math.lgamma(n))

    top = n * m + 40.0 * math.sqrt(n) * m
    return simpson(density, t, top, 4000)


def required_snr_db(pd_fn, n, pd, pfa, lo=-20.0, hi=40.0, iterations=50):
    t = threshold(n, pfa)
    return bisect(lambda s: pd_fn(n, undb(s), t) - pd, lo, hi, iterations)


def exact_pair(name, fn_a, fn_b, n, pd=0.9, pfa=1e-6):
    """Solve with method A, then solve with method B in a bracket around it."""
    a = required_snr_db(fn_a, n, pd, pfa)
    b = required_snr_db(fn_b, n, pd, pfa, a - 0.01, a + 0.01, 24)
    if abs(a - b) > 1e-4 or abs(b - (a - 0.01)) < 1e-6 or abs(b - (a + 0.01)) < 1e-6:
        sys.exit(f"{name}: the two methods disagree, {a:.6f} against {b:.6f} dB")
    return a, b


# --- Approximations ----------------------------------------------------------


def albersheim(pd, pfa, n):
    a = math.log(0.62 / pfa)
    b = math.log(pd / (1.0 - pd))
    return -5.0 * math.log10(n) + (6.2 + 4.54 / math.sqrt(n + 0.44)) * math.log10(
        a + 0.12 * a * b + 1.7 * b
    )


def shnidman(pd, pfa, n, swerling):
    """Shnidman 2002, as the MathWorks `shnidman` page states it."""
    k = {0: math.inf, 1: 1.0, 2: float(n), 3: 2.0, 4: 2.0 * n}[swerling]
    alpha = 0.0 if n < 40 else 0.25
    eta = math.sqrt(-0.8 * math.log(4.0 * pfa * (1.0 - pfa))) + math.copysign(
        math.sqrt(-0.8 * math.log(4.0 * pd * (1.0 - pd))), pd - 0.5
    )
    x_inf = eta * (eta + 2.0 * math.sqrt(n / 2.0 + (alpha - 0.25)))
    if math.isinf(k):
        c_db = 0.0
    else:
        c1 = (((17.7006 * pd - 18.4496) * pd + 14.5339) * pd - 3.525) / k
        c2 = (math.exp(27.31 * pd - 25.14) + (pd - 0.8) * (0.7 * math.log(1e-5 / pfa) + (2 * n - 20) / 80.0)) / k
        c_db = c1 if pd <= 0.872 else c1 + c2
    return db(undb(c_db) * x_inf / n)


def erfcinv(y):
    return bisect(lambda x: y - math.erfc(x), -10.0, 10.0)


def q_func(x):
    return 0.5 * math.erfc(x / math.sqrt(2.0))


def q_inv(p):
    return bisect(lambda x: p - q_func(x), -10.0, 40.0)


def coherent_pd_complex(snr_db, pfa):
    return 0.5 * math.erfc(erfcinv(2.0 * pfa) - math.sqrt(undb(snr_db)))


def coherent_pd_complex_quad(snr_db, pfa):
    """The same Pd by integrating the Gaussian density: an independent check."""
    x0 = math.sqrt(2.0) * erfcinv(2.0 * pfa) - math.sqrt(2.0 * undb(snr_db))
    pdf = lambda x: math.exp(-0.5 * x * x) / math.sqrt(2.0 * math.pi)
    return simpson(pdf, x0, 40.0, 20000)


# --- Link budgets ------------------------------------------------------------


def monostatic_snr_db(f, r, pt, tau, g_db, sigma, ts, loss_db, gr_db=None):
    lam = C / f
    gt = undb(g_db)
    gr = undb(g_db if gr_db is None else gr_db)
    return db(pt * tau * gt * gr * lam**2 * sigma / ((4 * math.pi) ** 3 * K_B * ts * r**4)) - loss_db


def monostatic_range(f, snr_db, pt, tau, g_db, sigma, ts, loss_db):
    lam = C / f
    g = undb(g_db)
    return (pt * tau * g * g * lam**2 * sigma / ((4 * math.pi) ** 3 * K_B * ts * undb(snr_db + loss_db))) ** 0.25


def main():
    out = []
    p = out.append

    p("== X: exact square-law detection (method A | method B), P_d 0.9 ==")
    d2b = exact_pair("D-2b", pd_sw0_series, pd_sw0_quad, 1)
    d3b = exact_pair("D-3b", pd_sw0_series, pd_sw0_quad, 10)
    d6b = exact_pair("D-6b", pd_sw2_closed, pd_sw2_quad, 10)
    d7b = exact_pair("D-7b", pd_sw1_closed, pd_sw1_quad, 10)
    d1x = exact_pair("D-1", pd_sw1_closed, pd_sw1_quad, 1)
    p(f"D-2b  Swerling 0, n 1,  P_fa 1e-6: {d2b[0]:.4f} | {d2b[1]:.4f} dB")
    p(f"D-3b  Swerling 0, n 10, P_fa 1e-6: {d3b[0]:.4f} | {d3b[1]:.4f} dB")
    p(f"D-6b  Swerling 2, n 10, P_fa 1e-6: {d6b[0]:.4f} | {d6b[1]:.4f} dB")
    p(f"D-7b  Swerling 1, n 10, P_fa 1e-6: {d7b[0]:.4f} | {d7b[1]:.4f} dB")
    gi = d2b[0] - d3b[0]
    p(f"D-9   exact integration gain n 1 -> 10: {gi:.3f} dB; loss 10 - G_i: {10.0 - gi:.2f} dB")
    p(f"D-9   exact fluctuation loss, Swerling 1 against 0, n 10: {d7b[0] - d3b[0]:.4f} dB")
    p(f"(D-1, V on the page: Swerling 1, n 1, both methods: {d1x[0]:.4f} | {d1x[1]:.4f} dB;"
      f" closed form ln P_fa / ln P_d - 1: {db(math.log(1e-6) / math.log(0.9) - 1.0):.4f} dB)")

    p("")
    p("== X: approximations evaluated (no published output for these inputs) ==")
    p(f"D-2c  Albersheim, P_fa 1e-6, n 1:  {albersheim(0.9, 1e-6, 1):.4f} dB")
    p(f"D-4   Albersheim, P_fa 0.01, n 1:  {albersheim(0.9, 0.01, 1):.4f} dB")
    p(f"D-5   Shnidman, Swerling 1, P_fa 1e-8, n 10: {shnidman(0.9, 1e-8, 10, 1):.4f} dB")
    p(f"D-7a  Shnidman, Swerling 1, P_fa 1e-6, n 10: {shnidman(0.9, 1e-6, 10, 1):.4f} dB")
    p(f"D-8   Shnidman fluctuation loss, n 10: {shnidman(0.9, 1e-6, 10, 1) - shnidman(0.9, 1e-6, 10, 0):.4f} dB")
    p(f"D-6a  Shnidman, Swerling 2, P_fa 1e-6, n 10: {shnidman(0.9, 1e-6, 10, 2):.4f} dB")
    pd_c = coherent_pd_complex(8.0, 0.01)
    pd_cq = coherent_pd_complex_quad(8.0, 0.01)
    p(f"D-10  coherent, complex signal, SNR 8 dB, P_fa 0.01: P_d {pd_c:.4f} | {pd_cq:.4f}")
    p(f"      (a real signal would give P_d {q_func(q_inv(0.01) - math.sqrt(undb(8.0))):.4f})")
    p(f"D-11  real-signal threshold 20 log10 Q^-1(P_fa), P_fa 1e-6: {20 * math.log10(q_inv(1e-6)):.4f} dB")

    p("")
    p("== Published values the approximations reproduce (V; shown for the check) ==")
    s0_1 = shnidman(0.9, 1e-6, 1, 0)
    s0_10 = shnidman(0.9, 1e-6, 10, 0)
    s1_10 = shnidman(0.9, 1e-6, 10, 1)
    p(f"D-2a  Shnidman, Swerling 0, n 1:  {s0_1:.4f} dB (page 13.1217)")
    p(f"D-3a  Albersheim, n 10:           {albersheim(0.9, 1e-6, 10):.4f} dB (page 4.9904)")
    p(f"D-8   Shnidman G_i, n 1 -> 10:    {s0_1 - s0_10:.4f} dB (page 7.7881)")
    p(f"D-11  P_fa 0.01:                  {20 * math.log10(q_inv(0.01)):.4f} dB (page 7.3335)")
    p(f"Shnidman, Swerling 1, n 10, P_fa 1e-6: {s1_10:.4f} dB; its fluctuation loss"
      f" against Shnidman Swerling 0: {s1_10 - s0_10:.4f} dB")
    p(f"  (the detectability page's 13.5033 and 8.1696 are Barton's universal equation;"
      f" 13.5033 - {s0_10:.4f} = {13.5033 - s0_10:.4f})")

    p("")
    p("== X: corrected and derived link budgets ==")
    k_lin = 1e5 * 1.0 * 9.0 * 10.0 * 1.0 / ((4 * math.pi) ** 3 * K_B * T0 * undb(5.0) * undb(10.0))
    k_db_sum = 50.0 + 0.0 + db(9.0) + db(10.0) + db(1.0) - 3 * db(4 * math.pi) - db(K_B * T0) - 5.0 - 10.0
    k_pub = k_db_sum + 5.0
    p(f"P-1   corrected K: {db(k_lin):.4f} | {k_db_sum:.4f} dB; published (no noise figure) {k_pub:.4f} dB")
    p(f"      equal-range 12 dB distance: corrected {10 ** ((db(k_lin) - 12.0) / 40.0) / 1e3:.1f} km;"
      f" from the published K {10 ** ((k_pub - 12.0) / 40.0) / 1e3:.1f} km")
    floor_p3 = db(K_B * T0 * undb(4.5) * 150e3)
    rr = math.sqrt(4000.0 * undb(2.2) * 9.0 * 100.0 / ((4 * math.pi) ** 3 * K_B * T0 * undb(4.5) * 150e3 * undb(10.0)))
    p(f"P-3   noise floor {floor_p3:.1f} dBW; R_T R_R at 10 dB {rr:.3e} m^2 (equal ranges"
      f" {math.sqrt(rr) / 1e3:.1f} km); with G_p 47 dB, equal ranges {math.sqrt(rr * undb(47.0) ** 0.5) / 1e3:.0f} km")
    p(f"      O'Hagan's interference scenario: k T0 F B at F 25 dB, B 150 kHz = {db(K_B * T0 * undb(25.0) * 150e3) + 30.0:.1f} dBm")

    p("")
    p("== V-calc: published outputs reproduced ==")
    p(f"R-1   {monostatic_snr_db(1e9, 50e3, 1e6, 0.2e-6, 20.0, 1.0, 290.0, 0.0):.5f} dB")
    p(f"R-2   {monostatic_snr_db(10e9, 100e3, 1e6, 1e-6, 40.0, 0.5, 300.0, 3.0):.5f} dB")
    p(f"R-3   {monostatic_range(10e9, 6.0, 1e6, 10e-6, 20.0, 1.0, 290.0, 0.0):.1f} m")
    p(f"R-4   {monostatic_range(10e9, 6.0, 1e6, 10e-6, 40.0, 0.1, 290.0, 3.0):.1f} m")
    ts5 = T0 * undb(4.1)
    p(f"R-5   systemp(4.1) {ts5:.2f} K; SNR at 100 km {monostatic_snr_db(3e9, 100e3, 0.2e6, 11e-6, 34.0, 1.0, ts5, 0.0):.4f} dB;"
      f" range at 13.5033 dB {monostatic_range(3e9, 13.5033, 0.2e6, 11e-6, 34.0, 1.0, ts5, 0.0) / 1e3:.4f} km")
    lam6 = C / 10e9
    a6 = albersheim(0.9, 1e-6, 10)
    b6 = C / (2 * 50.0)  # noise bandwidth for a 50 m range resolution
    pt6 = lambda snr_db: (4 * math.pi) ** 3 * K_B * T0 * b6 * 5000.0**4 * undb(snr_db) / (100.0 * 100.0 * lam6**2 * 1.0)
    p(f"R-6   Albersheim {a6:.5f} dB; peak power {pt6(a6):.2f} W (from the rounded 4.9904: {pt6(4.9904):.2f} W);"
      f" PRF {C / (2 * 5000.0) / 1e3:.2f} kHz")
    p(f"B-1   {monostatic_snr_db(10e9, 1.0, 1e6, 1e-6, 40.0, 1.0, 290.0, 0.0, gr_db=20.0) - db(50e3**2 * 75e3**2):.5f} dB")
    pd = lambda eirp, r: db(eirp / (4 * math.pi * r * r))
    p(f"B-2   FM {pd(250e3, 100e3):.2f} dBW/m2; TV {pd(1e6, 100e3):.2f} dBW/m2")
    p(f"B-3   ERP 50 kW used as EIRP, 400 km: {pd(50e3, 400e3):.2f} dBW/m2 (as EIRP + 2.15 dB: {pd(50e3, 400e3) + 2.15:.2f})")
    p(f"B-4   c/2B: FM 50 kHz {C / (2 * 50e3):.2f} m; TV 5.5 MHz {C / (2 * 5.5e6):.2f} m")
    p(f"P-2   B T = 50 kHz x 1 s: {db(50e3):.2f} dB")
    floors = [("FM", 75e3), ("GSM", 200e3), ("UMTS", 3.84e6), ("DAB", 1.536e6), ("DVB-T", 7.61e6)]
    p("P-4   " + "; ".join(f"{n} {db(K_B * 300.0 * b) + 30.0:.2f} dBm" for n, b in floors)
      + f"; FM direct at 10 km {db(100e3 * 3.0**2 / ((4 * math.pi * 10e3) ** 2)) + 30.0:.2f} dBm")
    ke_re = 4.0 / 3.0 * 6371e3
    hz = lambda h: math.sqrt(2 * ke_re * h + h * h)
    p(f"L-1   {hz(30.0) / 1e3:.3f} km (100 m: {hz(100.0) / 1e3:.1f} km)")
    p(f"L-2   {4.12 * (math.sqrt(30.0) + math.sqrt(1000.0)):.2f} km; L-3 {4.12 * (math.sqrt(313.0) + math.sqrt(1000.0)):.2f} km")
    p(f"C-1   {10 ** -0.5 * math.sin(math.radians(20.0)):.5f}")
    p(f"M-1   {35.76 / (C / 24.15e9):.2f} Hz")
    prf1, prf2 = C / (2 * 6000.0), C / (2 * 5000.0)
    p(f"M-3   PRFs {prf1 / 1e3:.3f} and {prf2 / 1e3:.3f} kHz; first common blind Doppler"
      f" 6 x {prf1 / 1e3:.3f} = 5 x {prf2 / 1e3:.3f} = {6 * prf1 / 1e3:.3f} | {5 * prf2 / 1e3:.3f} kHz")

    p("")
    p("== Memo figures computed here ==")
    lam_fm = C / 96.8e6
    p(f"4.3   1 Hz Doppler at 96.8 MHz: bistatic range rate {lam_fm:.2f} m/s; monostatic equivalent {lam_fm / 2:.2f} m/s")
    p(f"8.2   k T0 = {K_B * T0:.2e} W/Hz; in 1 MHz {K_B * T0 * 1e6:.2e} W")
    p(f"8.5   1024-entry log table, 20 log10 over R^2's mantissa: truncated {20 * math.log10(1 + 1 / 1024):.4f} dB;"
      f" centred {20 * math.log10(1 + 1 / 2048):.4f} dB")
    p(f"8.7   2^24 Complex64 samples: {2**24 * 16 / 2**20:.0f} MiB")

    p("")
    p("== Section 5.5: idealised solar integration time (not a golden) ==")
    d_req = undb(13.18)  # the steady-target threshold as section 5.5 states it
    # The table's fluxes: one station, or the mean of two rounded to two
    # figures (80.5 printed 80, 120.5 printed 120).
    bands = [(245e6, 15.0), (610e6, 57.0), (1415e6, 80.0), (2695e6, 120.0)]
    kt = K_B * 300.0
    for f, sfu in bands:
        s = sfu * 1e-22
        ref = 0.5 * s * 1.0 / kt
        derate = ref / (1.0 + ref)
        cells = []
        for r in (300.0, 1e3, 3e3, 10e3):
            echo = 0.5 * s * 10.0 * 1.0 / (4 * math.pi * r * r) / kt
            t = d_req / (echo * 10e6 * derate)
            cells.append(f"{t:9.1f} s")
        p(f"  {f / 1e6:6.0f} MHz {sfu:6.1f} sfu: " + " ".join(cells))
    theta = math.radians(0.53)
    p(f"  extended Sun (0.53 deg): coherence holds while d_perp < c / (B theta) = {C / (10e6 * theta) / 1e3:.2f} km at 10 MHz")
    lam245 = C / 245e6
    p(f"  1 m^2 aperture at 245 MHz: gain 4 pi A / lambda^2 = {db(4 * math.pi / lam245**2):.1f} dBi")
    p(f"  range cell c/2B at 10 MHz: {C / 2e7:.1f} m; at 2 m/s it is crossed in {C / 2e7 / 2.0:.1f} s")

    print("\n".join(out))


if __name__ == "__main__":
    main()
```
