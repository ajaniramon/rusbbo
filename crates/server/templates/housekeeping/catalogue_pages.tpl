{% set title = locale.hk_paginas_del_catalogo %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="toolbar">
  <input type="search" data-filter="pages-table" placeholder="{{ locale.hk_filtrar_paginas_por_nombre_id_o_layout|raw }}">
  <span class="spacer"></span>
  <a class="btn primary" href="{{ hk.path }}/catalogue/pages/edit">{{ locale.hk_nueva_pagina|raw }}</a>
</div>

<div class="card flush">
  <div class="card-head"><h2>{{ locale.hk_arbol_del_catalogo|raw }} <span class="sub">{{ tree | length }} {{ locale.hk_paginas_2|raw }}</span></h2><span class="muted small">{{ locale.hk_cada_cambio_recarga_el_catalogo_del_juego_al_mom|raw }}</span></div>
  <div class="card-body scroll">
    {% if tree is empty %}<div class="empty">{{ locale.hk_no_hay_paginas_en_el_catalogo|raw }}</div>{% else %}
    <table class="tbl" id="pages-table">
      <thead><tr><th>{{ locale.hk_pagina_2|raw }}</th><th>{{ locale.hk_id|raw }}</th><th>{{ locale.hk_layout|raw }}</th><th>{{ locale.hk_rango_minimo|raw }}</th><th>{{ locale.hk_opciones|raw }}</th><th class="num">{{ locale.hk_articulos|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for row in tree %}
        {% set page = row.page %}
        <tr>
          <td class="{% if row.depth == 1 %}indent-1{% elseif row.depth == 2 %}indent-2{% elseif row.depth >= 3 %}indent-3{% endif %}">
            {% if row.depth > 0 %}<span class="muted">&#8627;</span> {% endif %}<a href="{{ hk.path }}/catalogue/pages/edit?id={{ page.id() }}"><strong>{{ page.name() }}</strong></a>
            {% if row.orphan %}<span class="pill warn" title="{{ locale.hk_su_pagina_padre_no_existe|raw }}">{{ locale.hk_huerfana|raw }}</span>{% endif %}
          </td>
          <td class="muted">#{{ page.id() }}</td>
          <td class="mono">{{ page.layout() }}</td>
          <td>{% if page.minRole() > 1 %}<span class="pill accent">{{ hk.rankLabel(page.minRole()) }}</span>{% else %}<span class="muted">{{ hk.rankLabel(page.minRole()) }}</span>{% endif %}</td>
          <td>
            {% if page.navigatable() %}<span class="pill ok">{{ locale.hk_visible|raw }}</span>{% else %}<span class="pill">{{ locale.hk_oculta|raw }}</span>{% endif %}
            {% if page.clubOnly() %}<span class="pill info">{{ locale.hk_solo_hc|raw }}</span>{% endif %}
          </td>
          <td class="num">{% if row.items > 0 %}<a href="{{ hk.path }}/catalogue/items?page={{ page.id() }}">{{ row.items }}</a>{% else %}<span class="muted">0</span>{% endif %}</td>
          <td class="actions">
            <a class="btn sm" href="{{ hk.path }}/catalogue/pages/edit?id={{ page.id() }}">{{ locale.hk_editar|raw }}</a>
            <form class="inline" method="post" action="{{ hk.path }}/catalogue/pages/delete?id={{ page.id() }}" data-confirm="{{ locale.hk_borrar_la_pagina|raw }}{{ page.name() }}{{ locale.hk_del_catalogo_sus_articulos_quedaran_sin_pagina|raw }}">
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
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
