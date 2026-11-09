{% set title = locale.hk_coleccionables %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="toolbar">
  <span class="muted">{{ locale.hk_ciclos_de_furnis_coleccionables_cada_cierto_tiem|raw }}</span>
  <span class="spacer"></span>
  <a class="btn primary" href="{{ hk.path }}/catalogue/collectables/edit">{{ locale.hk_nuevo_ciclo|raw }}</a>
</div>

<div class="card flush">
  <div class="card-body scroll">
    {% if collectables is empty %}<div class="empty">{{ locale.hk_no_hay_ciclos_de_coleccionables|raw }}</div>{% else %}
    <table class="tbl">
      <thead><tr><th>{{ locale.hk_id|raw }}</th><th>{{ locale.hk_pagina_tienda|raw }}</th><th>{{ locale.hk_pagina_admin|raw }}</th><th>{{ locale.hk_caduca_timestamp|raw }}</th><th class="num">{{ locale.hk_duracion_s|raw }}</th><th class="num">{{ locale.hk_posicion|raw }}</th><th>{{ locale.hk_furnis_clases|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for collectable in collectables %}
        <tr>
          <td class="muted">#{{ collectable.id() }}</td>
          <td><a href="{{ hk.path }}/catalogue/pages/edit?id={{ collectable.storePageId() }}">#{{ collectable.storePageId() }}</a></td>
          <td><a href="{{ hk.path }}/catalogue/pages/edit?id={{ collectable.adminPageId() }}">#{{ collectable.adminPageId() }}</a></td>
          <td class="mono">{{ collectable.expiry() }}</td>
          <td class="num">{{ collectable.lifetime() }}</td>
          <td class="num">{{ collectable.currentPosition() }}</td>
          <td class="mono small wrap">{{ collectable.classNames() }}</td>
          <td class="actions">
            <a class="btn sm" href="{{ hk.path }}/catalogue/collectables/edit?id={{ collectable.id() }}">{{ locale.hk_editar|raw }}</a>
            <form class="inline" method="post" action="{{ hk.path }}/catalogue/collectables/delete?id={{ collectable.id() }}" data-confirm="{{ locale.hk_borrar_este_ciclo_de_coleccionables|raw }}">
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
