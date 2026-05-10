import streamlit as st
from datetime import datetime
from btc_fear_greed import get_btc_fear_greed_plot
from btc_halving import get_btc_halving_plot
from onchain_indicator import get_onchain_plot
from cycle_indicator import get_cycle_plot
from sth_sopr import get_sth_sopr_plot
from investment_simulator import run_simulation, get_simulator_plot

st.set_page_config(page_title="Crypto & Finance Dashboard", layout="wide")

st.title("📊 Tableau de Bord d'Indicateurs Financiers")

st.sidebar.title("Navigation")
selection = st.sidebar.selectbox(
    "Choisissez un indicateur",
    ["Accueil", "BTC Fear & Greed Index", "BTC Halving", "On-chain Indicators", "Bitcoin 4-Year Cycle", "SOPR (LTH/STH)", "Simulateur d'Investissement"]
)

scale_type = st.sidebar.radio(
    "Type d'échelle (Axe Y)",
    ["Linéaire", "Logarithmique"],
    index=1 if selection in ["BTC Halving", "On-chain Indicators"] else 0
)
yaxis_type = "log" if scale_type == "Logarithmique" else "linear"

if selection == "Accueil":
    st.write("## Bienvenue sur votre interface d'analyse financière.")
    st.write("""
    Cette application permet de visualiser différents indicateurs sur les marchés crypto et financiers.

    Utilisez le menu à gauche pour sélectionner l'indicateur que vous souhaitez afficher.

    ### Indicateurs disponibles :
    - **BTC Fear & Greed Index** : Visualisez le prix du Bitcoin coloré selon l'indice de peur et de cupidité.
    - **BTC Halving** : Suivez les halvings passés, les sommets/creux de cycle et l'estimation du prochain halving.
    - **On-chain Indicators** : Identifiez les zones de prix extrêmes avec des moyennes mobiles historiques (200w SMA, Realized Price, Pi Cycle).
    - **Bitcoin 4-Year Cycle** : Une roue psychologique pour visualiser l'évolution du prix à travers les cycles de 4 ans.
    - **SOPR (LTH/STH)** : Ratio de profit des détenteurs à court et long terme via Dune Analytics.
    - **Simulateur d'Investissement** : Testez une stratégie de levier dynamique basée sur les baisses de prix.
    """)

elif selection == "Simulateur d'Investissement":
    st.write("## Simulateur d'Investissement à Levier Dynamique")

    col1, col2, col3 = st.columns(3)
    with col1:
        start_date = st.date_input("Date de début", value=datetime(2017, 1, 1))
    with col2:
        end_date = st.date_input("Date de fin", value=datetime.now())
    with col3:
        drop_pct = st.number_input("Baisse pour déclencher le levier (%)", value=10.0, step=1.0)

    target_lev = st.sidebar.slider("Effet de levier cible", 1.1, 5.0, 2.0, 0.1)

    if start_date >= end_date:
        st.error("La date de début doit être antérieure à la date de fin.")
    else:
        with st.spinner("Simulation en cours..."):
            history_df, _ = run_simulation(start_date, end_date, drop_pct, target_lev)
            if history_df is not None:
                fig = get_simulator_plot(history_df)
                st.plotly_chart(fig, use_container_width=True)

                # Stats
                final_equity = history_df['Equity'].iloc[-1]
                bh_equity = history_df['BuyHold'].iloc[-1]
                perf = (final_equity / 10000.0 - 1) * 100
                st.write(f"### Résultat de la stratégie :")
                st.write(f"- Capital Final Stratégie : **{final_equity:,.2f} USD** ({perf:+.2f}%)")
                st.write(f"- Capital Final Buy & Hold : **{bh_equity:,.2f} USD**")
            else:
                st.error("Pas de données disponibles pour cette période.")

elif selection == "BTC Fear & Greed Index":
    st.write("## BTC Fear & Greed Index")
    try:
        with st.spinner("Chargement des données en cours..."):
            fig = get_btc_fear_greed_plot()
            if fig:
                fig.update_layout(yaxis_type=yaxis_type)
                st.plotly_chart(fig, use_container_width=True)
                st.write("**Interprétation :** Rouge = Peur extrême, Vert = Cupidité extrême.")
            else:
                st.error("Impossible de récupérer les données.")
    except Exception as e:
        st.error(f"Erreur : {e}")

elif selection == "On-chain Indicators":
    st.write("## On-chain Top & Bottom Indicators")
    try:
        with st.spinner("Calcul des indicateurs On-chain..."):
            fig = get_onchain_plot()
            if fig:
                fig.update_layout(yaxis_type=yaxis_type)
                st.plotly_chart(fig, use_container_width=True)
            else:
                st.error("Impossible de récupérer les données.")
    except Exception as e:
        st.error(f"Erreur : {e}")

elif selection == "Bitcoin 4-Year Cycle":
    st.write("## Bitcoin 4-Year Cycle (Hodler's Cheat Sheet)")
    with st.spinner("Génération du cycle de 4 ans..."):
        fig = get_cycle_plot()
        if fig:
            st.plotly_chart(fig, use_container_width=True)
        else:
            st.error("Impossible de charger les données du cycle.")

elif selection == "BTC Halving":
    st.write("## BTC Halving")
    try:
        with st.spinner("Récupération des données BTC + Halvings..."):
            fig = get_btc_halving_plot()
            if fig:
                fig.update_layout(yaxis_type=yaxis_type)
                st.plotly_chart(fig, use_container_width=True)
            else:
                st.error("Impossible de récupérer les données.")
    except Exception as e:
        st.error(f"Erreur : {e}")

elif selection == "SOPR (LTH/STH)":
    st.write("## Bitcoin SOPR (LTH & STH)")
    try:
        with st.spinner("Récupération des données Dune Analytics..."):
            fig = get_sth_sopr_plot()
            if fig:
                st.plotly_chart(fig, use_container_width=True)
                st.write("""
                **Interprétation du SOPR :**
                - **SOPR > 1** : Les investisseurs réalisent des profits.
                - **SOPR < 1** : Les investisseurs vendent à perte.
                - **Ligne orange (1.0)** : Niveau psychologique clé (Support en Bull market, Résistance en Bear market).
                """)
            else:
                st.error("Impossible de récupérer les données SOPR.")
    except Exception as e:
        st.error(f"Erreur : {e}")
