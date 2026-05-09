import matplotlib.pyplot as plt
import numpy as np
import streamlit as st

def get_cycle_plot():
    plt.style.use('dark_background')
    fig = plt.figure(figsize=(12, 12))
    ax = fig.add_subplot(111, projection='polar')

    # ====================== Phases (angles en radians) ======================
    phases = [
        ("EUPHORIA",          0,          "I'm a genius."),
        ("COMPLACENCY",       np.pi/6,    "We just need to cool off for the next rally."),
        ("DENIAL",            np.pi/3,    "Why am I getting margin calls?"),
        ("PANIC",             np.pi/2,    "Shit, everyone is selling."),
        ("CAPITULATION",      2*np.pi/3,  "I can't afford to lose."),
        ("DEPRESSION",        5*np.pi/6,  "I'm an idiot."),
        ("ENDURING",          np.pi,      "This is going nowhere."),
        ("STAGNATION",        7*np.pi/6,  "This is a sucker's rally."),
        ("DISBELIEF",         4*np.pi/3,  "Fail like before."),
        ("DOUBT",             3*np.pi/2,  "Is the halving priced in?"),
        ("HOPE",              5*np.pi/3,  "A recovery is possible."),
        ("OPTIMISM",          11*np.pi/6, "Let's wait for confirmation."),
        ("BELIEF",            2*np.pi,    "Time to get fully invested.")
    ]

    # Affichage des labels
    for label, angle, sublabel in phases:
        ax.text(angle, 1.45, label, ha='center', va='center', fontsize=10, fontweight='bold', color='white')
        ax.text(angle, 1.25, sublabel, ha='center', va='center', fontsize=7, color='#AAAAAA', rotation=angle*180/np.pi - 90)

    # ====================== Cercles de prix (log) ======================
    price_levels = [100, 1000, 10000, 100000, 1000000]
    for p in price_levels:
        radius = np.log10(p)
        ax.plot(np.linspace(0, 2*np.pi, 200), [radius]*200, color='gray', linewidth=0.8, alpha=0.4)
        ax.text(0, radius, f"${p:,.0f}", color='white', fontsize=9, ha='right', va='center')

    # ====================== Courbes des cycles ======================
    theta = np.linspace(0, 2*np.pi, 400)

    # Cycle 1 (2011-2015) - rouge foncé
    cycle1 = 4.5 + 2.2 * np.sin(3 * theta)
    ax.plot(theta, cycle1, color='#8B0000', linewidth=2, alpha=0.9, label="2011-2015")

    # Cycle 2 (2015-2019) - orange
    cycle2 = 5.2 + 2.4 * np.sin(3 * theta + 0.4)
    ax.plot(theta, cycle2, color='#FF8800', linewidth=2, alpha=0.9, label="2015-2019")

    # Cycle 3 (2019-2023) - jaune
    cycle3 = 6.0 + 2.6 * np.sin(3 * theta + 0.9)
    ax.plot(theta, cycle3, color='#FFDD00', linewidth=2, alpha=0.9, label="2019-2023")

    # Cycle actuel (2023-2027) - vert
    cycle4 = 6.8 + 2.8 * np.sin(3 * theta + 1.3)
    ax.plot(theta, cycle4, color='#00FF88', linewidth=3, label="2023-2027")

    # ====================== Titre et légende ======================
    ax.set_title("BITCOIN 4-YEAR CYCLE\nHodler's Cheat Sheet", fontsize=16, pad=60, color='white')

    # Légende Bear / Bull
    cbar_ax = fig.add_axes([0.88, 0.75, 0.02, 0.15])
    cbar = plt.colorbar(plt.cm.ScalarMappable(cmap=plt.cm.RdYlGn_r), cax=cbar_ax)
    cbar.set_label('STH Cost Basis Z-Score', color='white', fontsize=8)
    cbar.ax.tick_params(colors='white', labelsize=7)

    ax.text(0, 0, "2025", fontsize=14, ha='center', va='center', color='white', fontweight='bold')

    ax.set_rmax(8.5)
    ax.set_rlabel_position(0)
    ax.set_theta_direction(-1)
    ax.set_theta_zero_location('N')
    ax.grid(True, color='gray', alpha=0.3)

    plt.tight_layout()
    return fig

if __name__ == "__main__":
    fig = get_cycle_plot()
    plt.show()
