from unittest.mock import MagicMock, patch

import pandas as pd

from data_provider import get_dune_query_results, get_ticker_history


def _mock_response(status_code, json_data=None, text=""):
    resp = MagicMock()
    resp.status_code = status_code
    resp.json.return_value = json_data or {}
    resp.text = text
    return resp


def test_missing_api_key_returns_error():
    df, error = get_dune_query_results("12345", api_key="")
    assert df is None
    assert "Clé API Dune manquante" in error


@patch("data_provider.requests.get")
def test_success_returns_dataframe(mock_get):
    mock_get.return_value = _mock_response(200, {"result": {"rows": [{"a": 1}, {"a": 2}]}})
    df, error = get_dune_query_results("12345", api_key="fake-key")
    assert error is None
    assert isinstance(df, pd.DataFrame)
    assert len(df) == 2


@patch("data_provider.requests.get")
def test_unauthorized_returns_error(mock_get):
    mock_get.return_value = _mock_response(401)
    df, error = get_dune_query_results("12345", api_key="fake-key")
    assert df is None
    assert "401" in error


@patch("data_provider.requests.get")
def test_not_found_returns_error(mock_get):
    mock_get.return_value = _mock_response(404)
    df, error = get_dune_query_results("12345", api_key="fake-key")
    assert df is None
    assert "404" in error


@patch("data_provider.requests.get")
def test_bad_request_includes_details(mock_get):
    mock_get.return_value = _mock_response(400, {"error": "colonne invalide"})
    df, error = get_dune_query_results("12345", api_key="fake-key")
    assert df is None
    assert "colonne invalide" in error


@patch("data_provider.yf.download")
def test_get_ticker_history_flattens_multiindex(mock_download):
    dates = pd.date_range("2021-01-01", periods=3)
    columns = pd.MultiIndex.from_product([["Close", "Open"], ["BTC-USD"]])
    df = pd.DataFrame([[100, 99], [101, 100], [102, 101]], index=dates, columns=columns)
    mock_download.return_value = df

    result = get_ticker_history("BTC-USD", start="2021-01-01")

    assert not isinstance(result.columns, pd.MultiIndex)
    assert "Close" in result.columns


@patch("data_provider.yf.download")
def test_get_ticker_history_returns_empty_df_on_exception(mock_download):
    mock_download.side_effect = Exception("network error")
    result = get_ticker_history("ETH-USD", start="2022-06-01")
    assert result.empty
