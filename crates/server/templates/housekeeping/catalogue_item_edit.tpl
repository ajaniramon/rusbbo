{% set title = (item != null) ? (locale.hk_editar_articulo ~ item.saleCode()) : locale.hk_nuevo_articulo_del_catalogo %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="page-head">
  <div><p>{% if item != null %}{{ locale.hk_articulo|raw }}{{ item.id() }}. {% endif %}{{ locale.hk_al_guardar_se_recarga_el_catalogo_del_juego_al_m|raw }}</p></div>
  <div class="actions"><a class="btn" href="{{ hk.path }}/catalogue/items">{{ locale.hk_volver_a_los_articulos|raw }}</a></div>
</div>

<div class="card">
  <div class="card-body">
    <form class="form" method="post" action="{{ hk.path }}/catalogue/items/edit{% if item != null %}?id={{ item.id() }}{% endif %}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <div class="row r-2">
        <div class="field"><label>{{ locale.hk_codigo_de_venta|raw }}</label><input type="text" name="sale_code" class="mono" required value="{% if item != null %}{{ item.saleCode() }}{% endif %}"><span class="help">{{ locale.hk_identificador_unico_del_articulo_tambien_lo_usan|raw }}</span></div>
        <div class="field">
          <label>{{ locale.hk_paginas_ids_separados_por_comas|raw }}</label>
          <input type="text" name="page_id" id="page_id" class="mono" required value="{% if item != null %}{{ item.pageId() }}{% endif %}" placeholder="{{ locale.hk_p_ej_5_o_5_12|raw }}">
          <select id="page_picker" style="margin-top:4px">
            <option value="">{{ locale.hk_anadir_una_pagina|raw }}</option>
            {% for p in pages %}<option value="{{ p.id() }}">{{ p.name() }} (#{{ p.id() }})</option>{% endfor %}
          </select>
        </div>
      </div>
      <div class="row">
        <div class="field"><label>{{ locale.hk_precio_en_creditos|raw }}</label><input type="number" name="price_coins" min="0" value="{% if item != null %}{{ item.priceCoins() }}{% else %}3{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_precio_en_pixeles|raw }}</label><input type="number" name="price_pixels" min="0" value="{% if item != null %}{{ item.pricePixels() }}{% else %}0{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_cantidad|raw }}</label><input type="number" name="amount" min="1" value="{% if item != null %}{{ item.amount() }}{% else %}1{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_orden|raw }}</label><input type="number" name="order_id" value="{% if item != null %}{{ item.orderId() }}{% else %}0{% endif %}"></div>
      </div>
      <div class="row">
        <div class="field"><label>{{ locale.hk_creditos_en_temporada|raw }}</label><input type="number" name="seasonal_coins" min="0" value="{% if item != null %}{{ item.seasonalCoins() }}{% else %}0{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_pixeles_en_temporada|raw }}</label><input type="number" name="seasonal_pixels" min="0" value="{% if item != null %}{{ item.seasonalPixels() }}{% else %}0{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_id_de_definicion_de_furni|raw }}</label><input type="number" name="definition_id" value="{% if item != null %}{{ item.definitionId() }}{% else %}0{% endif %}"><span class="help">{% if item != null %}<a href="{{ hk.path }}/item_definitions/edit?id={{ item.definitionId() }}">{{ locale.hk_ver_definicion|raw }}{{ item.definitionId() }}</a>{% else %}{{ locale.hk_buscalo_en|raw }} <a href="{{ hk.path }}/item_definitions">{{ locale.hk_definiciones_de_furni|raw }}</a>{% endif %}</span></div>
        <div class="field"><label>{{ locale.hk_activo_desde|raw }}</label><input type="text" name="active_at" placeholder="{{ locale.hk_yyyy_mm_dd_hh_mm_ss_vacio_siempre|raw }}" value="{% if item != null and item.activeAt() != null %}{{ item.activeAt() }}{% endif %}"></div>
      </div>
      <div class="field"><label>{{ locale.hk_sprite_especial_poster_papel_suelo|raw }}</label><input type="text" name="item_specialspriteid" class="mono" value="{% if item != null %}{{ item.itemSpecialSpriteId() }}{% endif %}"></div>
      <div class="form-actions">
        <label class="check"><input type="checkbox" name="hidden" {% if item != null and item.hidden() %}checked{% endif %}> {{ locale.hk_oculto_en_el_catalogo|raw }}</label>
        <label class="check"><input type="checkbox" name="is_package" {% if item != null and item.packageItem() %}checked{% endif %}> {{ locale.hk_es_un_pack_ver_packs|raw }}</label>
      </div>
      <div class="form-actions"><button class="btn primary" type="submit">{% if item != null %}{{ locale.hk_guardar_articulo|raw }}{% else %}{{ locale.hk_crear_articulo|raw }}{% endif %}</button></div>
    </form>
  </div>
</div>

{% if item != null %}
<div class="card danger-zone mt">
  <div class="card-head"><h2>{{ locale.hk_zona_peligrosa|raw }}</h2></div>
  <div class="card-body form-actions">
    <form class="inline" method="post" action="{{ hk.path }}/catalogue/items/delete?id={{ item.id() }}" data-confirm="{{ locale.hk_borrar_el_articulo|raw }} {{ item.saleCode() }}{{ locale.hk_no_se_puede_deshacer|raw }}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <button class="btn danger" type="submit">{{ locale.hk_borrar_articulo|raw }}</button>
    </form>
    <span class="help">{{ locale.hk_si_solo_quieres_que_deje_de_venderse_marca_ocult|raw }}</span>
  </div>
</div>
{% endif %}
<script>
(function () {
  var picker = document.getElementById('page_picker');
  var field = document.getElementById('page_id');
  if (!picker || !field) return;
  picker.addEventListener('change', function () {
    var id = picker.value;
    if (!id) return;
    var parts = field.value.split(',').map(function (s) { return s.trim(); }).filter(function (s) { return s.length > 0; });
    if (parts.indexOf(id) === -1) parts.push(id);
    field.value = parts.join(',');
    picker.value = '';
  });
})();
</script>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
