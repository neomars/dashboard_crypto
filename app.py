import streamlit as st
from btc_fear_greed import get_btc_fear_greed_plot
from btc_halving import get_btc_halving_plot
from onchain_indicator import get_onchain_plot
from cycle_indicator import get_cycle_plot

st.set_page_config(page_title="Crypto & Finance Dashboard", layout="wide")

st.title("📊 Tableau de Bord d'Indicateurs Financiers")

st.sidebar.title("Navigation")
selection = st.sidebar.selectbox(
    "Choisissez un indicateur",
    ["Accueil", "BTC Fear & Greed Index", "BTC Halving", "On-chain Indicators", "Bitcoin 4-Year Cycle"]
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
    """)

elif selection == "BTC Fear & Greed Index":
    st.write("## BTC Fear & Greed Index")

    try:
        with st.spinner("Chargement des données en cours... Cela peut prendre quelques instants."):
            fig = get_btc_fear_greed_plot()
            if fig:
                fig.update_layout(yaxis_type=yaxis_type)
                st.plotly_chart(fig, use_container_width=True)
                st.write("""
                **Interprétation :**
                - **Rouge** : Peur extrême (Extreme Fear) - Peut être une opportunité d'achat.
                - **Vert** : Cupidité extrême (Extreme Greed) - Peut être un signe de correction imminente.
                """)
            else:
                st.error("Impossible de récupérer les données pour le moment.")
    except Exception as e:
        st.error(f"Une erreur est survenue : {e}")

elif selection == "On-chain Indicators":
    st.write("## On-chain Top & Bottom Indicators")

    try:
        with st.spinner("Calcul des indicateurs On-chain..."):
            fig = get_onchain_plot()
            if fig:
                fig.update_layout(yaxis_type=yaxis_type)
                st.plotly_chart(fig, use_container_width=True)
                st.write("""
                **Indicateurs inclus :**
                - **200-week SMA** : Souvent considéré comme le support ultime en marché baissier.
                - **Realized Price** : Prix moyen d'achat de tous les Bitcoins sur le réseau (approximation).
                - **Pi Cycle Top** : Utilise le croisement de deux moyennes mobiles pour identifier les sommets de cycle.
                - **2-year SMA Multiplier** : Aide à identifier les zones de surachat et survente.
                """)
            else:
                st.error("Impossible de récupérer les données pour le moment.")
    except Exception as e:
        st.error(f"Une erreur est survenue : {e}")

elif selection == "Bitcoin 4-Year Cycle":
    st.write("## Bitcoin 4-Year Cycle (Hodler's Cheat Sheet)")
    with st.spinner("Génération du cycle de 4 ans..."):
        fig = get_cycle_plot()
        if fig:
            st.plotly_chart(fig, use_container_width=True)
        else:
            st.error("Impossible de charger les données du cycle.")
    st.write("""
    **Interprétation :**
    Ce graphique polaire interactif représente les cycles de 4 ans du Bitcoin.
    - **Survol** : Passez la souris sur les points pour voir la date, le prix et le nombre de jours après le dernier halving.
    - **Secteurs** : Chaque quart du cercle représente une année du cycle de 4 ans.
    - **Phases** : Les étiquettes extérieures indiquent les phases psychologiques classiques du marché.
    - L'échelle radiale est logarithmique (prix du BTC).
    """)

elif selection == "BTC Halving":
    st.write("## BTC Halving")

    try:
        with st.spinner("Récupération des données BTC + Halvings..."):
            fig = get_btc_halving_plot()
            if fig:
                fig.update_layout(yaxis_type=yaxis_type)
                st.plotly_chart(fig, use_container_width=True)
                st.write("""
                **À propos du Halving :**
                Le halving du Bitcoin est un événement qui divise par deux la récompense de minage de nouveaux blocs.
                Il a lieu environ tous les 4 ans (tous les 210 000 blocs) et réduit l'offre de nouveaux Bitcoins, ce qui a historiquement eu un impact sur son prix.
                """)
            else:
                st.error("Impossible de récupérer les données pour le moment.")
    except Exception as e:
        st.error(f"Une erreur est survenue : {e}")
