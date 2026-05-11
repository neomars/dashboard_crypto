import streamlit as st
import json
import importlib
from datetime import datetime

# --- Chargement de la configuration ---
def load_indicators():
    with open('indicators.json', 'r', encoding='utf-8') as f:
        return json.load(f)

INDICATORS = load_indicators()

st.set_page_config(page_title="Crypto & Finance Dashboard", layout="wide")
st.title("📊 Tableau de Bord d'Indicateurs Financiers")

# --- Navigation ---
st.sidebar.title("Navigation")
menu_options = ["Accueil"] + [ind["name"] for ind in INDICATORS]
selection = st.sidebar.selectbox("Choisissez un indicateur", menu_options)

# --- Configuration de l'échelle ---
selected_ind = next((ind for ind in INDICATORS if ind["name"] == selection), None)

default_scale_idx = 0
if selected_ind and selected_ind.get("default_scale") == "logarithmique":
    default_scale_idx = 1

scale_type = st.sidebar.radio(
    "Type d'échelle (Axe Y)",
    ["Linéaire", "Logarithmique"],
    index=default_scale_idx
)
yaxis_type = "log" if scale_type == "Logarithmique" else "linear"

# --- Contenu Principal ---
if selection == "Accueil":
    st.write("## Bienvenue sur votre interface d'analyse financière.")
    st.write("Cette application permet de visualiser différents indicateurs sur les marchés crypto et financiers.")
    st.write("### Indicateurs disponibles :")
    for ind in INDICATORS:
        st.write(f"- **{ind['name']}** : {ind['description']}")

else:
    st.write(f"## {selected_ind['name']}")

    # Cas particulier : Simulateur d'Investissement
    if selected_ind.get("id") == "simulator":
        from investment_simulator import run_simulation

        # Interface de saisie principale
        col1, col2 = st.columns(2)
        with col1:
            start_date = st.date_input("Date de début", value=datetime(2017, 1, 1))
            initial_capital = st.number_input("Investissement initial (USD)", value=10000, step=1000)
        with col2:
            end_date = st.date_input("Date de fin", value=datetime.now())
            target_lev = st.number_input("Effet de levier cible", value=2.0, step=0.1, min_value=1.0)

        drop_pct = st.slider("Baisse pour déclencher le levier (%)", 1.0, 50.0, 10.0, 0.5)

        if start_date >= end_date:
            st.error("La date de début doit être antérieure à la date de fin.")
        else:
            with st.spinner("Simulation en cours..."):
                module = importlib.import_module(selected_ind["module"])
                sim_func = getattr(module, "run_simulation")
                plot_func = getattr(module, selected_ind["function"])

                history_df = sim_func(start_date, end_date, initial_capital, drop_pct, target_lev)
                if history_df is not None:
                    fig = plot_func(history_df)
                    st.plotly_chart(fig, use_container_width=True)

                    final_equity = history_df['Portfolio_Value'].iloc[-1]
                    bh_equity = history_df['Buy_Hold'].iloc[-1]
                    perf = (final_equity / initial_capital - 1) * 100
                    st.write(f"### Résultat de la stratégie :")
                    st.write(f"- Capital Final Stratégie : **{final_equity:,.2f} USD** ({perf:+.2f}%)")
                    st.write(f"- Capital Final Buy & Hold : **{bh_equity:,.2f} USD**")
                else:
                    st.error("Pas de données disponibles.")

    # Cas standard pour les autres indicateurs
    else:
        try:
            with st.spinner(f"Chargement de {selected_ind['name']}..."):
                module = importlib.import_module(selected_ind["module"])
                plot_func = getattr(module, selected_ind["function"])

                fig = plot_func()
                if fig:
                    if hasattr(fig, 'update_layout'):
                        fig.update_layout(yaxis_type=yaxis_type)
                    st.plotly_chart(fig, use_container_width=True)

                    if selected_ind.get("interpretation"):
                        st.write(f"**Interprétation :** {selected_ind['interpretation']}")
                else:
                    st.error("Impossible de générer le graphique.")
        except Exception as e:
            st.error(f"Erreur lors du chargement de l'indicateur : {e}")
