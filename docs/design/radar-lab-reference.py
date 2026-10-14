# SPDX-FileCopyrightText: 2026 Pharmakos contributors
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Reference arithmetic for docs/design/radar-lab-research.md.

Documentation, not game code: no build step, test or CI job runs it, and
nothing ships it. It reproduces every value the memo flags X (an exact or
formula value with no published output to compare against, confirmed by two
independent implementations) and the memo's own computed figures, so that a
reader can check them without the research's working.

Standard library only. Run it isolated:

    python -I docs/design/radar-lab-reference.py

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
