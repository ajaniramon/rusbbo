{% set title = locale.hk_categorias_del_navegador %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="toolbar">
  <input type="search" data-filter="cat-table" placeholder="{{ locale.hk_filtrar_categorias|raw }}">
  <span class="spacer"></span>
  <a class="btn primary" href="{{ hk.path }}/room_categories/edit">{{ locale.hk_nueva_categoria|raw }}</a>
</div>
<div class="card flush">
  <div class="card-body scroll">
    <table class="tbl" id="cat-table">
      <thead><tr><th>{{ locale.hk_id|raw }}</th><th>{{ locale.hk_nombre|raw }}</th><th>{{ locale.hk_padre|raw }}</th><th class="num">{{ locale.hk_orden|raw }}</th><th>{{ locale.hk_ver_crear_salas|raw }}</th><th>{{ locale.hk_tipo|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for c in categories %}
        <tr>
          <td class="muted">{{ c.id() }}</td>
          <td><strong>{{ c.name() }}</strong></td>
          <td class="muted">{% if c.parentId() <= 0 %}{{ locale.hk_raiz|raw }}{% else %}{% for p in categories %}{% if p.id() == c.parentId() %}{{ p.name() }}{% endif %}{% endfor %}{% endif %}</td>
          <td class="num">{{ c.orderId() }}</td>
          <td class="small">{{ hk.rankLabel(c.minRoleAccess()) }} / {{ hk.rankLabel(c.minRoleSetFlatCat()) }}</td>
          <td>
            {% if c.node() %}<span class="pill">{{ locale.hk_carpeta|raw }}</span>{% endif %}
            {% if c.publicSpaces() %}<span class="pill info">{{ locale.hk_publicas_2|raw }}</span>{% endif %}
            {% if c.allowTrading() %}<span class="pill ok">{{ locale.hk_tradeo|raw }}</span>{% endif %}
            {% if c.clubOnly() %}<span class="pill accent">{{ locale.hk_solo_hc|raw }}</span>{% endif %}
            {% if c.topPriority() %}<span class="pill warn">{{ locale.hk_prioritaria|raw }}</span>{% endif %}
          </td>
          <td class="actions">
            <a class="btn sm" href="{{ hk.path }}/room_categories/edit?id={{ c.id() }}">{{ locale.hk_editar|raw }}</a>
            <form class="inline" method="post" action="{{ hk.path }}/room_categories/delete?id={{ c.id() }}" data-confirm="{{ locale.hk_borrar_la_categoria|raw }}{{ c.name() }}{{ locale.hk_las_salas_que_la_usen_se_quedaran_sin_categoria|raw }}">
              <input type="hidden" name="hk_token" value="{{ hk.token }}"><button class="btn sm ghost-danger" type="submit">{{ locale.hk_borrar|raw }}</button>
            </form>
          </td>
        </tr>
      {% endfor %}
      </tbody>
    </table>
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
