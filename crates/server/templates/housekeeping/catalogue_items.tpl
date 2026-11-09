{% set title = locale.hk_articulos_y_precios %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="toolbar">
  <form method="get" action="{{ hk.path }}/catalogue/items">
    <input type="search" name="query" value="{{ query }}" placeholder="{{ locale.hk_codigo_de_venta_furni_o_id|raw }}">
    <select name="page">
      <option value="" {% if pageFilter == '' %}selected{% endif %}>{{ locale.hk_todas_las_paginas|raw }}</option>
      {% for p in pages %}
        <option value="{{ p.id() }}" {% if (p.id() ~ '') == pageFilter %}selected{% endif %}>{{ p.name() }} (#{{ p.id() }})</option>
      {% endfor %}
    </select>
    <button class="btn primary" type="submit">{{ locale.hk_buscar|raw }}</button>
    {% if query != '' or pageFilter != '' %}<a class="btn ghost" href="{{ hk.path }}/catalogue/items">{{ locale.hk_limpiar|raw }}</a>{% endif %}
  </form>
  <span class="spacer"></span>
  <a class="btn primary" href="{{ hk.path }}/catalogue/items/edit">{{ locale.hk_nuevo_articulo|raw }}</a>
</div>

<div class="card flush">
  <div class="card-body scroll">
    {% if items is empty %}<div class="empty">{{ locale.hk_no_hay_articulos_que_coincidan|raw }}</div>{% else %}
    <table class="tbl">
      <thead><tr><th>{{ locale.hk_id|raw }}</th><th>{{ locale.hk_codigo_de_venta|raw }}</th><th>{{ locale.hk_furni|raw }}</th><th>{{ locale.hk_pagina_s|raw }}</th><th class="num">{{ locale.hk_precio|raw }}</th><th class="num">{{ locale.hk_cant|raw }}</th><th>{{ locale.hk_opciones|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for row in items %}
        {% set item = row.item %}
        <tr>
          <td class="muted">#{{ item.id() }}</td>
          <td class="mono"><a href="{{ hk.path }}/catalogue/items/edit?id={{ item.id() }}">{{ item.saleCode() }}</a></td>
          <td class="wrap">{{ row.definition }}</td>
          <td class="wrap small">{{ row.pages }}</td>
          <td class="num">
            {% if item.priceCoins() > 0 %}{{ item.priceCoins() }} {{ locale.hk_c|raw }}{% endif %}{% if item.pricePixels() > 0 %} {{ item.pricePixels() }} {{ locale.hk_px|raw }}{% endif %}{% if item.priceCoins() == 0 and item.pricePixels() == 0 %}<span class="muted">{{ locale.hk_gratis|raw }}</span>{% endif %}
            {% if item.seasonalCoins() > 0 or item.seasonalPixels() > 0 %}<div class="muted small">{{ locale.hk_temporada|raw }} {{ item.seasonalCoins() }} {{ locale.hk_c_2|raw }} {{ item.seasonalPixels() }} {{ locale.hk_px|raw }}</div>{% endif %}
          </td>
          <td class="num">{{ item.amount() }}</td>
          <td>
            {% if item.hidden() %}<span class="pill warn">{{ locale.hk_oculto|raw }}</span>{% endif %}
            {% if item.packageItem() %}<span class="pill info">{{ locale.hk_pack|raw }}</span>{% endif %}
          </td>
          <td class="actions">
            <a class="btn sm" href="{{ hk.path }}/catalogue/items/edit?id={{ item.id() }}">{{ locale.hk_editar|raw }}</a>
            <form class="inline" method="post" action="{{ hk.path }}/catalogue/items/delete?id={{ item.id() }}" data-confirm="{{ locale.hk_borrar_el_articulo|raw }} {{ item.saleCode() }} {{ locale.hk_del_catalogo|raw }}">
              <input type="hidden" name="hk_token" value="{{ hk.token }}">
              <button class="btn sm ghost-danger" type="submit">{{ locale.hk_borrar|raw }}</button>
            </form>
          </td>
        </tr>
      {% endfor %}
      </tbody>
    </table>
    {% endif %}
  </div>
  {% include "housekeeping/base/pager.tpl" %}
</div>
<p class="help mt">{{ locale.hk_los_cambios_de_precio_se_aplican_en_el_juego_al|raw }}</p>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
