// Thème et recherche de la documentation ; le site reste lisible sans ce script.
(function () {
  "use strict";

  var racine = document.body.dataset.racine || "";

  var bouton = document.getElementById("theme");
  if (bouton) {
    bouton.addEventListener("click", function () {
      var sombre = document.documentElement.dataset.theme
        ? document.documentElement.dataset.theme === "dark"
        : matchMedia("(prefers-color-scheme: dark)").matches;
      var suivant = sombre ? "light" : "dark";
      document.documentElement.dataset.theme = suivant;
      try { localStorage.setItem("theme", suivant); } catch (e) { /* préférence facultative */ }
    });
  }

  // Téléchargement : l'archive du système du visiteur passe en premier, mise en avant.
  var plateforme = ((navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "").toLowerCase();
  var systeme = /win/.test(plateforme) ? "windows" : /mac/.test(plateforme) ? "macos"
    : /linux/.test(plateforme) && !/android/.test(navigator.userAgent.toLowerCase()) ? "linux" : "";
  var archive = systeme && document.querySelector('.telechargements [data-systeme="' + systeme + '"]');
  if (archive) {
    archive.classList.add("principal");
    archive.textContent = "Télécharger pour " + archive.textContent;
    archive.parentNode.insertBefore(archive, archive.parentNode.firstElementChild);
  }

  // Section lue, marquée dans le sommaire.
  var liens = document.querySelectorAll(".sommaire a");
  if (liens.length && "IntersectionObserver" in window) {
    var parId = {};
    liens.forEach(function (a) { parId[a.getAttribute("href").slice(1)] = a; });
    var observateur = new IntersectionObserver(function (vues) {
      vues.forEach(function (vue) {
        if (!vue.isIntersecting) return;
        liens.forEach(function (a) { a.removeAttribute("aria-current"); });
        var lien = parId[vue.target.id];
        if (lien) lien.setAttribute("aria-current", "true");
      });
    }, { rootMargin: "-10% 0px -80% 0px" });
    document.querySelectorAll("article h2[id], article h3[id]").forEach(function (h) { observateur.observe(h); });
  }

  var champ = document.getElementById("recherche");
  var liste = document.getElementById("resultats");
  if (!champ || !liste) return;

  var index = null;
  var chargement = null;
  var actif = -1;

  function normaliser(s) {
    return s.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
  }

  function charger() {
    if (!chargement) {
      chargement = fetch(racine + "recherche.json")
        .then(function (r) { if (!r.ok) throw new Error(String(r.status)); return r.json(); })
        .then(function (d) {
          index = d.map(function (e) { e.nt = normaliser(e.t); e.nx = normaliser(e.x); return e; });
        })
        .catch(function () { chargement = null; afficherMessage("Recherche indisponible."); });
    }
    return chargement;
  }

  function fermer() {
    liste.hidden = true;
    actif = -1;
    champ.setAttribute("aria-expanded", "false");
    champ.removeAttribute("aria-activedescendant");
  }

  function ouvrir() {
    liste.hidden = false;
    champ.setAttribute("aria-expanded", "true");
  }

  function afficherMessage(texte) {
    var li = document.createElement("li");
    li.className = "vide";
    li.textContent = texte;
    liste.replaceChildren(li);
    ouvrir();
  }

  function extrait(entree, mots) {
    var i = entree.nx.indexOf(mots[0]);
    if (i < 0) return entree.x.slice(0, 110);
    var debut = Math.max(0, i - 40);
    return (debut > 0 ? "… " : "") + entree.x.slice(debut, debut + 110);
  }

  function chercher() {
    var mots = normaliser(champ.value).split(/\s+/).filter(Boolean);
    if (!mots.length) { fermer(); return; }
    if (!index) { afficherMessage("Chargement…"); charger().then(function () { if (index) chercher(); }); return; }
    var trouves = index
      .map(function (e) {
        var score = 0;
        for (var i = 0; i < mots.length; i++) {
          var dansTitre = e.nt.indexOf(mots[i]) >= 0;
          if (!dansTitre && e.nx.indexOf(mots[i]) < 0) return null;
          score += dansTitre ? 10 : 1;
        }
        return { e: e, score: score };
      })
      .filter(Boolean)
      .sort(function (a, b) { return b.score - a.score; })
      .slice(0, 8);
    if (!trouves.length) { afficherMessage("Aucun résultat."); return; }
    liste.replaceChildren.apply(liste, trouves.map(function (t, i) {
      var li = document.createElement("li");
      var a = document.createElement("a");
      a.href = racine + t.e.u;
      a.id = "resultat-" + i;
      var titre = document.createElement("strong");
      titre.textContent = t.e.t;
      var page = document.createElement("small");
      page.textContent = t.e.p;
      var texte = document.createElement("span");
      texte.textContent = extrait(t.e, mots);
      a.append(titre, page, texte);
      li.append(a);
      return li;
    }));
    actif = -1;
    ouvrir();
  }

  function marquer(pas) {
    var a = liste.querySelectorAll("a");
    if (!a.length) return;
    if (actif >= 0) a[actif].classList.remove("actif");
    actif = (actif + pas + a.length) % a.length;
    a[actif].classList.add("actif");
    a[actif].scrollIntoView({ block: "nearest" });
    champ.setAttribute("aria-activedescendant", a[actif].id);
  }

  champ.addEventListener("focus", function () { charger(); if (champ.value) chercher(); });
  champ.addEventListener("input", chercher);
  champ.addEventListener("keydown", function (e) {
    if (e.key === "ArrowDown") { e.preventDefault(); marquer(1); }
    else if (e.key === "ArrowUp") { e.preventDefault(); marquer(-1); }
    else if (e.key === "Enter") {
      var a = liste.querySelectorAll("a");
      if (a.length) { e.preventDefault(); a[Math.max(actif, 0)].click(); }
    }
    else if (e.key === "Escape") { fermer(); champ.blur(); }
  });
  document.addEventListener("click", function (e) { if (!e.target.closest(".recherche")) fermer(); });
  document.addEventListener("keydown", function (e) {
    var t = e.target;
    var saisie = t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable;
    if (e.key === "/" && !saisie && !e.ctrlKey && !e.metaKey && !e.altKey) { e.preventDefault(); champ.focus(); }
  });
})();
