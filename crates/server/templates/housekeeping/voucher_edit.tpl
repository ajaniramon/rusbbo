{% set title = (voucher != null) ? (locale.hk_editar_vale ~ voucher.voucherCode()) : locale.hk_nuevo_vale %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="page-head">
  <div><p>{{ locale.hk_un_vale_puede_dar_creditos_furnis_del_catalogo_p|raw }}</p></div>
  <div class="actions"><a class="btn" href="{{ hk.path }}/vouchers">{{ locale.hk_volver_a_los_vales|raw }}</a></div>
</div>

<div class="card">
  <div class="card-body">
    <form class="form" method="post" action="{{ hk.path }}/vouchers/edit{% if voucher != null %}?code={{ voucher.voucherCode() }}{% endif %}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <div class="row r-3">
        <div class="field"><label>{{ locale.hk_codigo|raw }}</label><input type="text" name="voucher_code" class="mono" required value="{% if voucher != null %}{{ voucher.voucherCode() }}{% endif %}" placeholder="{{ locale.hk_p_ej_aeyra_fiesta|raw }}"></div>
        <div class="field"><label>{{ locale.hk_creditos_2|raw }}</label><input type="number" name="credits" min="0" value="{% if voucher != null %}{{ voucher.credits() }}{% else %}0{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_caduca|raw }}</label><input type="text" name="expiry_date" placeholder="{{ locale.hk_yyyy_mm_dd_hh_mm_ss_vacio_nunca|raw }}" value="{% if voucher != null and voucher.expiryDate() != null %}{{ voucher.expiryDate() }}{% endif %}"></div>
      </div>
      <div class="form-actions">
        <label class="check"><input type="checkbox" name="is_single_use" {% if voucher == null or voucher.singleUse() %}checked{% endif %}> {{ locale.hk_un_solo_uso_solo_lo_puede_canjear_una_persona|raw }}</label>
        <label class="check"><input type="checkbox" name="allow_new_users" {% if voucher != null and voucher.allowNewUsers() %}checked{% endif %}> {{ locale.hk_permitir_a_cuentas_recien_creadas|raw }}</label>
      </div>
      <div class="field">
        <label>{{ locale.hk_furnis_que_da_codigos_de_venta_uno_por_linea|raw }}</label>
        <textarea name="catalogue_sale_codes" rows="5" class="mono">{% for item in voucherItems %}{{ item.catalogueSaleCode() }}
{% endfor %}</textarea>
        <span class="help">{{ locale.hk_buscalos_en|raw }} <a href="{{ hk.path }}/catalogue/items">{{ locale.hk_articulos_y_precios|raw }}</a>.</span>
      </div>
      <div class="form-actions"><button class="btn primary" type="submit">{% if voucher != null %}{{ locale.hk_guardar_vale|raw }}{% else %}{{ locale.hk_crear_vale|raw }}{% endif %}</button></div>
    </form>
  </div>
</div>

{% if voucher != null %}
<div class="card flush mt">
  <div class="card-head"><h2>{{ locale.hk_canjes_de_este_vale|raw }}</h2></div>
  <div class="card-body scroll">
    {% if history is empty %}<div class="empty">{{ locale.hk_nadie_lo_ha_canjeado_todavia|raw }}</div>{% else %}
    <table class="tbl">
      <thead><tr><th>{{ locale.hk_cuando|raw }}</th><th>{{ locale.hk_usuario|raw }}</th><th class="num">{{ locale.hk_creditos_2|raw }}</th><th>{{ locale.hk_furnis|raw }}</th></tr></thead>
      <tbody>{% for row in history %}<tr><td class="nowrap muted">{{ row.usedAt() }}</td><td><a href="{{ hk.path }}/users/view?id={{ row.userId() }}">#{{ row.userId() }}</a></td><td class="num">{% if row.creditsRedeemed() != null %}{{ row.creditsRedeemed() }}{% else %}0{% endif %}</td><td class="mono small wrap">{{ row.itemsRedeemed() }}</td></tr>{% endfor %}</tbody>
    </table>
    {% endif %}
  </div>
</div>

<div class="card danger-zone mt">
  <div class="card-head"><h2>{{ locale.hk_zona_peligrosa|raw }}</h2></div>
  <div class="card-body">
    <form class="inline" method="post" action="{{ hk.path }}/vouchers/delete?code={{ voucher.voucherCode() }}" data-confirm="{{ locale.hk_borrar_el_vale|raw }} {{ voucher.voucherCode() }}?">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <button class="btn danger" type="submit">{{ locale.hk_borrar_vale|raw }}</button>
    </form>
  </div>
</div>
{% endif %}
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
