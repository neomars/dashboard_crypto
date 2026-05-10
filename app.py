import streamlit as st
from btc_fear_greed import get_btc_fear_greed_plot
from btc_halving import get_btc_halving_plot
from onchain_indicator import get_onchain_plot
from cycle_indicator import get_cycle_plot
from sth_sopr import get_sth_sopr_plot

st.set_page_config(page_title="Crypto & Finance Dashboard", layout="wide")

st.title("📊 Tableau de Bord d'Indicateurs Financiers")

st.sidebar.title("Navigation")
selection = st.sidebar.selectbox(
    "Choisissez un indicateur",
    ["Accueil", "BTC Fear & Greed Index", "BTC Halving", "On-chain Indicators", "Bitcoin 4-Year Cycle", "SOPR (LTH/STH)"]
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
    """)

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
