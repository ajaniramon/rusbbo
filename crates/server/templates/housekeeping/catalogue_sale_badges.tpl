{% set title = locale.hk_placas_por_compra %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="grid g-side">
  <div class="card flush">
    <div class="card-head"><h2>{{ locale.hk_recompensas|raw }} <span class="sub">{{ saleBadges | length }}</span></h2><span class="muted small">{{ locale.hk_quien_compra_ese_articulo_recibe_la_placa|raw }}</span></div>
    <div class="card-body scroll">
      {% if saleBadges is empty %}<div class="empty">{{ locale.hk_no_hay_placas_asociadas_a_compras|raw }}</div>{% else %}
      <table class="tbl">
        <thead><tr><th>{{ locale.hk_codigo_de_venta|raw }}</th><th>{{ locale.hk_placa|raw }}</th><th></th></tr></thead>
        <tbody>
        {% for reward in saleBadges %}
          <tr>
            <td class="mono"><a href="{{ hk.path }}/catalogue/items?query={{ reward.saleCode() }}">{{ reward.saleCode() }}</a></td>
            <td><div class="user-cell"><img class="badge-img" src="/c_images/album1584/{{ reward.badgeCode() }}.gif" alt="" onerror="this.style.visibility='hidden'"><span class="mono">{{ reward.badgeCode() }}</span></div></td>
            <td class="actions">
              <form class="inline" method="post" action="{{ hk.path }}/catalogue/sale_badges/delete?sale_code={{ reward.saleCode() }}&badge_code={{ reward.badgeCode() }}" data-confirm="{{ locale.hk_quitar_la_placa|raw }} {{ reward.badgeCode() }} {{ locale.hk_de_la_compra|raw }} {{ reward.saleCode() }}?">
                <input type="hidden" name="hk_token" value="{{ hk.token }}">
                <button class="btn sm ghost-danger" type="submit">{{ locale.hk_quitar|raw }}</button>
              </form>
            </td>
          </tr>
        {% endfor %}
        </tbody>
      </table>
      {% endif %}
    </div>
  </div>
  <div class="card">
    <div class="card-head"><h2>{{ locale.hk_anadir_recompensa|raw }}</h2></div>
    <div class="card-body">
      <form class="form" method="post" action="{{ hk.path }}/catalogue/sale_badges">
        <input type="hidden" name="hk_token" value="{{ hk.token }}">
        <div class="field"><label>{{ locale.hk_codigo_de_venta|raw }}</label><input type="text" name="sale_code" class="mono" required></div>
        <div class="field"><label>{{ locale.hk_codigo_de_placa|raw }}</label><input type="text" name="badge_code" class="mono" required placeholder="{{ locale.hk_p_ej_ach_login1|raw }}"></div>
        <div class="form-actions"><button class="btn primary" type="submit">{{ locale.hk_anadir|raw }}</button></div>
      </form>
    </div>
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
