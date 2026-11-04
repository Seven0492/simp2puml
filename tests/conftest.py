"""Réglages communs : les tests n'écrivent jamais dans le dossier de données réel de l'utilisateur."""
import pytest


@pytest.fixture(autouse=True)
def _no_user_preview_cache(monkeypatch):
    # Le cache disque des aperçus vit dans %LOCALAPPDATA% (ou équivalent) : désactivé pendant les tests,
    # qui utilisent un PreviewCache dans un dossier temporaire quand ils en ont besoin.
    monkeypatch.setenv("SIMP2PUML_NO_CACHE", "1")
