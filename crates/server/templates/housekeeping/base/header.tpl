{% autoescape 'html' %}<!DOCTYPE html>
<html lang="{{ hkLang }}">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="robots" content="noindex, nofollow">
  <title>{{ title | default(locale.hk_panel) }} · {{ site.siteName }} {{ locale.hk_housekeeping|raw }}</title>
  <link rel="shortcut icon" href="/housekeeping/favicon.ico" type="image/vnd.microsoft.icon">
  <link rel="stylesheet" href="/housekeeping/hk/hk.css?v=3">
</head>
<body class="hk">
{% if hk.loggedIn %}
<div class="layout">
  <aside class="sidebar" id="sidebar">
    <div class="brand">
      <div class="brand-mark">{{ locale.hk_a_2|raw }}</div>
      <div>
        <div class="brand-name">{{ site.siteName }} {{ locale.hk_hotel|raw }}</div>
        <div class="brand-sub">{{ locale.hk_housekeeping|raw }}</div>
      </div>
    </div>

    <nav class="nav">
      <div class="nav-group">
        <div class="nav-head">{{ locale.hk_general|raw }}</div>
        <a href="{{ hk.path }}" class="{% if hk.on('') %}on{% endif %}">{{ locale.hk_panel|raw }}</a>
        {% if hk.can('hotel/alert') %}<a href="{{ hk.path }}/hotel/alert" class="{% if hk.on('/hotel/alert') %}on{% endif %}">{{ locale.hk_alerta_al_hotel|raw }}</a>{% endif %}
        <a href="/" target="_blank" rel="noopener">{{ locale.hk_ver_la_web|raw }}</a>
      </div>

      {% if hk.can('user/search') or hk.can('user/create') or hk.can('badges') or hk.can('transaction/lookup') %}
      <div class="nav-group">
        <div class="nav-head">{{ locale.hk_usuarios|raw }}</div>
        {% if hk.can('user/search') %}<a href="{{ hk.path }}/users" class="{% if (hk.on('/users') and not hk.on('/users/create')) %}on{% endif %}">{{ locale.hk_usuarios|raw }}</a>{% endif %}
        {% if hk.can('user/create') %}<a href="{{ hk.path }}/users/create" class="{% if hk.on('/users/create') %}on{% endif %}">{{ locale.hk_crear_usuario|raw }}</a>{% endif %}
        {% if hk.can('badges') %}<a href="{{ hk.path }}/badges" class="{% if hk.on('/badges') %}on{% endif %}">{{ locale.hk_placas|raw }}</a>{% endif %}
        {% if hk.can('transaction/lookup') %}<a href="{{ hk.path }}/transaction/lookup" class="{% if hk.on('/transaction') %}on{% endif %}">{{ locale.hk_compras|raw }}</a>{% endif %}
      </div>
      {% endif %}

      {% if hk.can('chatlog') or hk.can('bans') or hk.can('audit') %}
      <div class="nav-group">
        <div class="nav-head">{{ locale.hk_moderacion|raw }}</div>
        {% if hk.can('chatlog') %}<a href="{{ hk.path }}/chatlogs" class="{% if hk.on('/chatlogs') %}on{% endif %}">{{ locale.hk_chat|raw }}</a>{% endif %}
        {% if hk.can('bans') %}<a href="{{ hk.path }}/bans" class="{% if hk.on('/bans') %}on{% endif %}">{{ locale.hk_baneos|raw }}</a>{% endif %}
        {% if hk.can('audit') %}<a href="{{ hk.path }}/audit" class="{% if hk.on('/audit') %}on{% endif %}">{{ locale.hk_registro_del_staff|raw }}</a>{% endif %}
      </div>
      {% endif %}

      {% if hk.can('rooms/view') or hk.can('room_categories/manage') or hk.can('room_models/manage') or hk.can('room_badges') or hk.can('room_ads') or hk.can('groups/manage') or hk.can('infobus') %}
      <div class="nav-group">
        <div class="nav-head">{{ locale.hk_salas|raw }}</div>
        {% if hk.can('rooms/view') %}<a href="{{ hk.path }}/rooms" class="{% if hk.on('/rooms') %}on{% endif %}">{{ locale.hk_salas|raw }}</a>{% endif %}
        {% if hk.can('room_categories/manage') %}<a href="{{ hk.path }}/room_categories" class="{% if hk.on('/room_categories') %}on{% endif %}">{{ locale.hk_categorias|raw }}</a>{% endif %}
        {% if hk.can('room_models/manage') %}<a href="{{ hk.path }}/room_models" class="{% if hk.on('/room_models') %}on{% endif %}">{{ locale.hk_modelos_de_sala|raw }}</a>{% endif %}
        {% if hk.can('room_badges') %}<a href="{{ hk.path }}/room_badges" class="{% if hk.on('/room_badges') %}on{% endif %}">{{ locale.hk_placas_de_entrada|raw }}</a>{% endif %}
        {% if hk.can('room_ads') %}<a href="{{ hk.path }}/room_ads" class="{% if hk.on('/room_ads') %}on{% endif %}">{{ locale.hk_anuncios|raw }}</a>{% endif %}
        {% if hk.can('groups/manage') %}<a href="{{ hk.path }}/groups" class="{% if hk.on('/groups') %}on{% endif %}">{{ locale.hk_grupos|raw }}</a>{% endif %}
        {% if hk.can('infobus') %}<a href="{{ hk.path }}/infobus_polls" class="{% if hk.on('/infobus_polls') %}on{% endif %}">{{ locale.hk_infobus|raw }}</a>{% endif %}
      </div>
      {% endif %}

      {% if hk.can('catalogue/manage') or hk.can('catalogue/edit_frontpage') or hk.can('item_definitions/manage') or hk.can('vouchers/manage') or hk.can('recycler/manage') %}
      <div class="nav-group">
        <div class="nav-head">{{ locale.hk_catalogo_y_economia|raw }}</div>
        {% if hk.can('catalogue/manage') %}
        <a href="{{ hk.path }}/catalogue/pages" class="{% if hk.on('/catalogue/pages') %}on{% endif %}">{{ locale.hk_paginas|raw }}</a>
        <a href="{{ hk.path }}/catalogue/items" class="{% if hk.on('/catalogue/items') %}on{% endif %}">{{ locale.hk_articulos_y_precios|raw }}</a>
        <a href="{{ hk.path }}/catalogue/packages" class="{% if hk.on('/catalogue/packages') %}on{% endif %}">{{ locale.hk_packs|raw }}</a>
        <a href="{{ hk.path }}/catalogue/sale_badges" class="{% if hk.on('/catalogue/sale_badges') %}on{% endif %}">{{ locale.hk_placas_por_compra|raw }}</a>
        <a href="{{ hk.path }}/catalogue/collectables" class="{% if hk.on('/catalogue/collectables') %}on{% endif %}">{{ locale.hk_coleccionables|raw }}</a>
        {% endif %}
        {% if hk.can('catalogue/edit_frontpage') %}<a href="{{ hk.path }}/catalogue/edit_frontpage" class="{% if hk.on('/catalogue/edit_frontpage') %}on{% endif %}">{{ locale.hk_portada_del_catalogo|raw }}</a>{% endif %}
        {% if hk.can('item_definitions/manage') %}<a href="{{ hk.path }}/item_definitions" class="{% if hk.on('/item_definitions') %}on{% endif %}">{{ locale.hk_definiciones_de_furni|raw }}</a>{% endif %}
        {% if hk.can('vouchers/manage') %}<a href="{{ hk.path }}/vouchers" class="{% if hk.on('/vouchers') %}on{% endif %}">{{ locale.hk_vales|raw }}</a>{% endif %}
        {% if hk.can('recycler/manage') %}<a href="{{ hk.path }}/recycler_rewards" class="{% if hk.on('/recycler_rewards') %}on{% endif %}">{{ locale.hk_reciclador|raw }}</a>{% endif %}
      </div>
      {% endif %}

      {% if hk.can('articles/create') or hk.can('wordfilter/manage') or hk.can('configuration') %}
      <div class="nav-group">
        <div class="nav-head">{{ locale.hk_web_y_ajustes|raw }}</div>
        {% if hk.can('articles/create') %}<a href="{{ hk.path }}/articles" class="{% if hk.on('/articles') %}on{% endif %}">{{ locale.hk_noticias|raw }}</a>{% endif %}
        {% if hk.can('wordfilter/manage') %}<a href="{{ hk.path }}/wordfilter" class="{% if hk.on('/wordfilter') %}on{% endif %}">{{ locale.hk_filtro_de_palabras|raw }}</a>{% endif %}
        {% if hk.can('configuration') %}<a href="{{ hk.path }}/configurations" class="{% if hk.on('/configurations') %}on{% endif %}">{{ locale.hk_configuracion|raw }}</a>{% endif %}
      </div>
      {% endif %}
    </nav>
  </aside>

  <div class="main">
    <header class="topbar">
      <button class="burger" type="button" onclick="document.body.classList.toggle('nav-open')" aria-label="{{ locale.hk_menu|raw }}">&#9776;</button>
      <h1>{{ title | default(locale.hk_panel) }}</h1>
      <div class="meta">
        {% if hk.serverOnline %}
          <span class="pill ok" title="{{ locale.hk_el_servidor_de_juego_responde_por_rcon|raw }}"><span class="dot ok"></span> {{ locale.hk_servidor_activo|raw }}</span>
        {% else %}
          <span class="pill bad" title="{{ locale.hk_el_servidor_de_juego_no_responde_por_rcon|raw }}"><span class="dot bad"></span> {{ locale.hk_servidor_caido|raw }}</span>
        {% endif %}
        <span class="hide-sm">{{ hk.usersOnline }} {{ locale.hk_conectado|raw }}{% if hk.usersOnline != 1 %}{{ locale.hk_pl_conectado|raw }}{% endif %}</span>
        <span class="hide-sm muted">·</span>
        <span class="hide-sm"><strong>{{ hk.staff.getName() }}</strong> <span class="muted">({{ hk.rankName }})</span></span>
        {% if hkLangSelector %}<span class="hide-sm lang-switch">{% if hkLang == 'es' %}<strong>ES</strong>{% else %}<a href="{{ hkLangUrls.es }}">ES</a>{% endif %} · {% if hkLang == 'en' %}<strong>EN</strong>{% else %}<a href="{{ hkLangUrls.en }}">EN</a>{% endif %}</span>
        {% endif %}<form method="post" action="{{ hk.path }}/logout">
          <input type="hidden" name="hk_token" value="{{ hk.token }}">
          <button class="btn sm" type="submit">{{ locale.hk_salir|raw }}</button>
        </form>
      </div>
    </header>
    <div class="content">
{% endif %}
{% if hk.loggedIn %}{% include "housekeeping/base/flash.tpl" %}{% endif %}
{% endautoescape %}
