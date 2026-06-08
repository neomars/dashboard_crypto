import streamlit as st
import json
import importlib
from datetime import datetime
from config_manager import (
    get_dune_api_key, save_dune_api_key, delete_dune_api_key
)

# --- Chargement de la configuration ---
def load_indicators():
    with open('indicators.json', 'r', encoding='utf-8') as f:
        return json.load(f)

INDICATORS = load_indicators()

st.set_page_config(page_title="Crypto & Finance Dashboard", layout="wide")

# Initialisation de l'état de navigation
if 'selection' not in st.session_state:
    st.session_state.selection = "Accueil"

st.title("📊 Tableau de Bord d'Indicateurs Financiers")

# --- Navigation latérale avec boutons ---
st.sidebar.title("🚀 Navigation")

# Bouton Accueil
if st.sidebar.button("🏠 Accueil", use_container_width=True):
    st.session_state.selection = "Accueil"

st.sidebar.markdown("---")

# Section Simulation
simulator = next((ind for ind in INDICATORS if ind.get("is_special")), None)
if simulator:
    st.sidebar.subheader("Simulation")
    if st.sidebar.button(f"{simulator['icon']} {simulator['name']}", use_container_width=True):
        st.session_state.selection = simulator['name']
    st.sidebar.markdown("---")

# Section Indicateurs
st.sidebar.subheader("Indicateurs")
other_indicators = [ind for ind in INDICATORS if not ind.get("is_special")]
for ind in other_indicators:
    if st.sidebar.button(f"{ind['icon']} {ind['name']}", use_container_width=True):
        st.session_state.selection = ind['name']

# --- Configuration de l'échelle (pour les graphiques compatibles) ---
selected_ind = next((ind for ind in INDICATORS if ind["name"] == st.session_state.selection), None)

if selected_ind:
    st.sidebar.markdown("---")
    default_scale_idx = 1 if selected_ind.get("default_scale") == "logarithmique" else 0
    scale_type = st.sidebar.radio(
        "Type d'échelle (Axe Y)",
        ["Linéaire", "Logarithmique"],
        index=default_scale_idx
    )
    yaxis_type = "log" if scale_type == "Logarithmique" else "linear"

# --- Contenu Principal ---
selection = st.session_state.selection

if selection == "Accueil":
    st.write("## Bienvenue sur votre interface d'analyse financière.")
    st.write("Cette application permet de visualiser différents indicateurs sur les marchés crypto et financiers.")

    # --- Configuration API keys ---
    st.write("### 🔑 Configuration")

    with st.expander("Configurer l'API Dune Analytics"):
        st.info("Les indicateurs basés sur Dune (SOPR, Institutional Holdings, Long/Short Positions) nécessitent une clé API. [dune.com](https://dune.com).")

        current_key_dune = get_dune_api_key()
        new_key_dune = st.text_input("Clé API Dune", value=current_key_dune, type="password", key="dune_key_input")

        c_save, c_del = st.columns([1, 2])
        with c_save:
            if st.button("Sauvegarder Dune", use_container_width=True):
                save_dune_api_key(new_key_dune)
                st.success("Clé Dune sauvegardée !")
                st.cache_data.clear()
                st.rerun()
        with c_del:
            if st.button("Supprimer Dune", use_container_width=True):
                delete_dune_api_key()
                st.warning("Clé Dune supprimée")
                st.cache_data.clear()
                st.rerun()

    st.write("### Explorez nos outils via la barre latérale :")

    col1, col2 = st.columns(2)
    with col1:
        if simulator:
            st.info(f"**{simulator['name']}** : {simulator['description']}")
    with col2:
        for ind in other_indicators[:3]:
            st.write(f"- **{ind['name']}** : {ind['description']}")

    for ind in other_indicators[3:]:
        st.write(f"- **{ind['name']}** : {ind['description']}")

else:
    header_text = f"## {selected_ind['icon']} {selected_ind['name']}"
    if selected_ind.get("id") == "simulator":
        header_text += " *(Btc est la valeur par défaut, aucune position en stable coin)*"
    st.write(header_text)

    # Cas particulier : Simulateur d'Investissement
    if selected_ind.get("id") == "simulator":
        from investment_simulator import run_simulation, generate_pdf_report

        # Interface de saisie principale
        col1, col2, col_extra = st.columns([1, 1, 1])
        with col1:
            start_date = st.date_input("Date de début", value=datetime(2017, 1, 1))
            initial_capital = st.number_input("Investissement initial (USD)", value=10000.0, step=100.0)
        with col2:
            end_date = st.date_input("Date de fin", value=datetime.now())
            target_lev = st.number_input("Effet de levier cible", value=2.0, step=0.1, min_value=1.0)
        with col_extra:
            ticker = st.text_input("Ticker Yahoo Finance", value="BTC-USD")
            st.info("Exemples: BTC-USD, ETH-USD, SOL-USD, AAPL, GC=F")

        col3, col4, col5 = st.columns(3)
        with col3:
            drop_pct = st.slider("Baisse déclencheur (%)", 1.0, 50.0, 10.0, 0.5)
        with col4:
            exit_freq = st.selectbox("Fréquence de sortie", ["Journalière", "Hebdomadaire", "Mensuelle"], index=1)
        with col5:
            exit_pct = st.number_input("Sortie par étape (%)", value=10.0, step=1.0, min_value=1.0, max_value=100.0)

        if start_date >= end_date:
            st.error("La date de début doit être antérieure à la date de fin.")
        else:
            with st.spinner("Simulation en cours..."):
                module = importlib.import_module(selected_ind["module"])
                sim_func = getattr(module, "run_simulation")
                plot_func = getattr(module, selected_ind["function"])

                history_df, trades_df = sim_func(
                    start_date, end_date, initial_capital, drop_pct, target_lev,
                    exit_frequency=exit_freq, exit_pct=exit_pct, ticker=ticker
                )
                if history_df is not None:
                    fig = plot_func(history_df, trades_df)
                    st.plotly_chart(fig, use_container_width=True)

                    final_equity = history_df['Portfolio_Value'].iloc[-1]
                    final_btc = history_df['BTC_Units'].iloc[-1]
                    initial_btc = initial_capital / history_df['BTC_Price'].iloc[0]

                    bh_equity = history_df['Buy_Hold'].iloc[-1]
                    max_dd = history_df['Drawdown'].min()

                    perf = (final_equity / initial_capital - 1) * 100
                    st.write(f"### 📈 Résultat de la stratégie :")
                    c1, c2, c3 = st.columns(3)
                    with c1:
                        st.metric("Capital Final (USD)", f"{final_equity:,.2f} $", f"{perf:+.2f} %")
                        st.write(f"*(Initial: {initial_capital:,.2f} $)*")
                    with c2:
                        st.metric("Capital Final (BTC)", f"{final_btc:.4f} BTC")
                        st.write(f"*(Initial: {initial_btc:.4f} BTC)*")
                    with c3:
                        st.metric("Drawdown Max", f"{max_dd:.2f} %", delta_color="inverse")

                    st.write(f"**Comparaison Buy & Hold :** {bh_equity:,.2f} USD")

                    # Alerte de liquidation
                    if not trades_df.empty and any(trades_df['Action'].str.contains('LIQUIDATION')):
                        st.error("💀 **ALERTE : Votre stratégie a été liquidée !** Le capital est tombé à zéro suite aux pertes sous levier.")

                    if not trades_df.empty:
                        st.write("### 📝 Journal des Opérations")
                        trades_display = trades_df.copy()
                        if 'Date' in trades_display.columns:
                            trades_display['Date'] = trades_display['Date'].dt.strftime('%Y-%m-%d')
                        st.dataframe(trades_display, use_container_width=True, hide_index=True)

                    # Export Section
                    st.write("### 📥 Exporter les résultats")
                    exp1, exp2 = st.columns(2)
                    with exp1:
                        csv = history_df.to_csv(index=False).encode('utf-8')
                        st.download_button(
                            label="Télécharger CSV (Historique)",
                            data=csv,
                            file_name=f"simulation_{ticker}_{datetime.now().strftime('%Y%m%d')}.csv",
                            mime='text/csv',
                        )
                    with exp2:
                        params = {
                            "Ticker": ticker,
                            "Date de début": start_date,
                            "Date de fin": end_date,
                            "Capital Initial": initial_capital,
                            "Levier Cible": target_lev,
                            "Baisse Déclencheur": f"{drop_pct}%",
                            "Fréquence Sortie": exit_freq,
                            "Sortie par étape": f"{exit_pct}%"
                        }
                        pdf_data = generate_pdf_report(history_df, trades_df, params)
                        st.download_button(
                            label="Télécharger Rapport PDF",
                            data=pdf_data,
                            file_name=f"rapport_simulation_{ticker}.pdf",
                            mime='application/pdf',
                        )
                else:
                    st.error("Pas de données disponibles.")

    # Cas standard pour les autres indicateurs
    else:
        try:
            with st.spinner(f"Chargement de {selected_ind['name']}..."):
                module = importlib.import_module(selected_ind["module"])
                plot_func = getattr(module, selected_ind["function"])

                # Appel de la fonction de génération
                fig = plot_func()

                if fig:
                    # Application du type d'échelle sélectionné dans la barre latérale
                    if hasattr(fig, 'update_layout'):
                        fig.update_layout(yaxis_type=yaxis_type)

                    # Rendu du graphique
                    st.plotly_chart(fig, use_container_width=True)

                    # Affichage de l'interprétation si disponible
                    if selected_ind.get("interpretation"):
                        st.info(f"💡 **Interprétation :** {selected_ind['interpretation']}")
                else:
                    st.error(f"Impossible de générer le graphique pour {selected_ind['name']}. Vérifiez les sources de données ou votre configuration API.")
        except Exception as e:
            st.error(f"Erreur lors du chargement de l'indicateur : {e}")
