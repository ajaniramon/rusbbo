{% set title = locale.hk_definiciones_de_furni %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="toolbar">
  <form method="get" action="{{ hk.path }}/item_definitions">
    <input type="search" name="query" value="{{ query }}" placeholder="{{ locale.hk_sprite_nombre_o_id|raw }}">
    <select name="interactor">
      <option value="" {% if interactor == '' %}selected{% endif %}>{{ locale.hk_todos_los_interactores|raw }}</option>
      {% for i in interactors %}<option value="{{ i }}" {% if i == interactor %}selected{% endif %}>{{ i }}</option>{% endfor %}
    </select>
    <button class="btn primary" type="submit">{{ locale.hk_buscar|raw }}</button>
    {% if query != '' or interactor != '' %}<a class="btn ghost" href="{{ hk.path }}/item_definitions">{{ locale.hk_limpiar|raw }}</a>{% endif %}
  </form>
  <span class="spacer"></span>
  <a class="btn primary" href="{{ hk.path }}/item_definitions/edit">{{ locale.hk_nueva_definicion|raw }}</a>
</div>

<div class="card flush">
  <div class="card-body scroll">
    {% if definitions is empty %}<div class="empty">{{ locale.hk_no_hay_definiciones_que_coincidan|raw }}</div>{% else %}
    <table class="tbl">
      <thead><tr><th>{{ locale.hk_id|raw }}</th><th>{{ locale.hk_sprite|raw }}</th><th>{{ locale.hk_nombre|raw }}</th><th class="num">{{ locale.hk_sprite_id|raw }}</th><th>{{ locale.hk_tamano|raw }}</th><th>{{ locale.hk_interactor|raw }}</th><th>{{ locale.hk_opciones|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for d in definitions %}
        <tr>
          <td class="muted">#{{ d.id() }}</td>
          <td class="mono"><a href="{{ hk.path }}/item_definitions/edit?id={{ d.id() }}">{{ d.sprite() }}</a></td>
          <td class="wrap">{{ d.name() }}</td>
          <td class="num">{{ d.spriteId() }}</td>
          <td class="nowrap">{{ d.length() }}{{ locale.hk_text|raw }}{{ d.width() }}</td>
          <td class="mono small">{{ d.interactor() }}</td>
          <td>
            {% if d.tradable() %}<span class="pill ok">{{ locale.hk_tradeable|raw }}</span>{% else %}<span class="pill">{{ locale.hk_no_tradeable|raw }}</span>{% endif %}
            {% if d.recyclable() %}<span class="pill info">{{ locale.hk_reciclable|raw }}</span>{% endif %}
          </td>
          <td class="actions">
            <a class="btn sm" href="{{ hk.path }}/catalogue/items?query={{ d.sprite() }}" title="{{ locale.hk_articulos_del_catalogo_con_este_furni|raw }}">{{ locale.hk_en_catalogo|raw }}</a>
            <a class="btn sm" href="{{ hk.path }}/item_definitions/edit?id={{ d.id() }}">{{ locale.hk_editar|raw }}</a>
          </td>
        </tr>
      {% endfor %}
      </tbody>
    </table>
    {% endif %}
  </div>
  {% include "housekeeping/base/pager.tpl" %}
</div>
<p class="help mt">{{ locale.hk_borrar_una_definicion_puede_dejar_furnis_huerfan|raw }}</p>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
