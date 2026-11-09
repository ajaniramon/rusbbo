{% set title = locale.hk_vales %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="toolbar">
  <input type="search" data-filter="vouchers-table" placeholder="{{ locale.hk_filtrar_vales|raw }}">
  <span class="spacer"></span>
  <a class="btn primary" href="{{ hk.path }}/vouchers/edit">{{ locale.hk_nuevo_vale_2|raw }}</a>
</div>

<div class="card flush">
  <div class="card-head"><h2>{{ locale.hk_vales_canjeables|raw }} <span class="sub">{{ vouchers | length }}</span></h2><span class="muted small">{{ locale.hk_se_canjean_en_la_web_credits_o_en_el_catalogo_de|raw }}</span></div>
  <div class="card-body scroll">
    {% if vouchers is empty %}<div class="empty">{{ locale.hk_no_hay_vales_crea_uno_para_regalar_creditos_o_fu|raw }}</div>{% else %}
    <table class="tbl" id="vouchers-table">
      <thead><tr><th>{{ locale.hk_codigo|raw }}</th><th class="num">{{ locale.hk_creditos_2|raw }}</th><th>{{ locale.hk_caduca|raw }}</th><th>{{ locale.hk_uso|raw }}</th><th>{{ locale.hk_cuentas_nuevas|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for voucher in vouchers %}
        <tr>
          <td class="mono"><a href="{{ hk.path }}/vouchers/edit?code={{ voucher.voucherCode() }}"><strong>{{ voucher.voucherCode() }}</strong></a></td>
          <td class="num">{{ voucher.credits() }}</td>
          <td class="nowrap">{% if voucher.expiryDate() != null and voucher.expiryDate() != '' %}{{ voucher.expiryDate() }}{% else %}<span class="muted">{{ locale.hk_nunca|raw }}</span>{% endif %}</td>
          <td>{% if voucher.singleUse() %}<span class="pill">{{ locale.hk_un_solo_uso|raw }}</span>{% else %}<span class="pill info">{{ locale.hk_varios_usos|raw }}</span>{% endif %}</td>
          <td>{% if voucher.allowNewUsers() %}<span class="pill ok">{{ locale.hk_permitido|raw }}</span>{% else %}<span class="pill">{{ locale.hk_no|raw }}</span>{% endif %}</td>
          <td class="actions">
            <a class="btn sm" href="{{ hk.path }}/vouchers/edit?code={{ voucher.voucherCode() }}">{{ locale.hk_editar|raw }}</a>
            <form class="inline" method="post" action="{{ hk.path }}/vouchers/delete?code={{ voucher.voucherCode() }}" data-confirm="{{ locale.hk_borrar_el_vale|raw }} {{ voucher.voucherCode() }}?">
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

<div class="card flush mt">
  <div class="card-head"><h2>{{ locale.hk_ultimos_canjes|raw }}</h2></div>
  <div class="card-body scroll">
    {% if history is empty %}<div class="empty">{{ locale.hk_nadie_ha_canjeado_un_vale_todavia|raw }}</div>{% else %}
    <table class="tbl">
      <thead><tr><th>{{ locale.hk_cuando|raw }}</th><th>{{ locale.hk_vale|raw }}</th><th>{{ locale.hk_usuario|raw }}</th><th class="num">{{ locale.hk_creditos_2|raw }}</th><th>{{ locale.hk_furnis|raw }}</th></tr></thead>
      <tbody>
      {% for row in history %}
        <tr>
          <td class="nowrap muted">{{ row.usedAt() }}</td>
          <td class="mono"><a href="{{ hk.path }}/vouchers/edit?code={{ row.voucherCode() }}">{{ row.voucherCode() }}</a></td>
          <td><a href="{{ hk.path }}/users/view?id={{ row.userId() }}">#{{ row.userId() }}</a></td>
          <td class="num">{% if row.creditsRedeemed() != null %}{{ row.creditsRedeemed() }}{% else %}0{% endif %}</td>
          <td class="mono small wrap">{{ row.itemsRedeemed() }}</td>
        </tr>
      {% endfor %}
      </tbody>
    </table>
    {% endif %}
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
