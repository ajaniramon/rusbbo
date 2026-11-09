{% set title = locale.hk_packs_del_catalogo %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="toolbar">
  <input type="search" data-filter="packages-table" placeholder="{{ locale.hk_filtrar_por_codigo_de_venta_o_furni|raw }}">
  <span class="spacer"></span>
  <a class="btn primary" href="{{ hk.path }}/catalogue/packages/edit">{{ locale.hk_nueva_fila_de_pack_2|raw }}</a>
</div>

<div class="card flush">
  <div class="card-head"><h2>{{ locale.hk_contenido_de_los_packs|raw }} <span class="sub">{{ packages | length }} {{ locale.hk_filas|raw }}</span></h2><span class="muted small">{{ locale.hk_cada_fila_es_un_furni_dentro_del_pack_con_ese_co|raw }}</span></div>
  <div class="card-body scroll">
    {% if packages is empty %}<div class="empty">{{ locale.hk_no_hay_packs_definidos|raw }}</div>{% else %}
    <table class="tbl" id="packages-table">
      <thead><tr><th>{{ locale.hk_id|raw }}</th><th>{{ locale.hk_codigo_de_venta_del_pack|raw }}</th><th>{{ locale.hk_definicion|raw }}</th><th>{{ locale.hk_sprite_especial|raw }}</th><th class="num">{{ locale.hk_cantidad|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for package in packages %}
        <tr>
          <td class="muted">#{{ package.id() }}</td>
          <td class="mono"><a href="{{ hk.path }}/catalogue/items?query={{ package.saleCode() }}">{{ package.saleCode() }}</a></td>
          <td><a href="{{ hk.path }}/item_definitions/edit?id={{ package.definitionId() }}">#{{ package.definitionId() }}</a></td>
          <td class="mono">{{ package.specialSpriteId() }}</td>
          <td class="num">{{ package.amount() }}</td>
          <td class="actions">
            <a class="btn sm" href="{{ hk.path }}/catalogue/packages/edit?id={{ package.id() }}">{{ locale.hk_editar|raw }}</a>
            <form class="inline" method="post" action="{{ hk.path }}/catalogue/packages/delete?id={{ package.id() }}" data-confirm="{{ locale.hk_borrar_esta_fila_del_pack_2|raw }} {{ package.saleCode() }}?">
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
