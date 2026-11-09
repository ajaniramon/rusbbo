{% set title = (cataloguePackage != null) ? (locale.hk_editar_fila_de_pack ~ cataloguePackage.saleCode()) : locale.hk_nueva_fila_de_pack %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="page-head">
  <div><p>{{ locale.hk_un_pack_es_un_articulo_del_catalogo_marcado_como|raw }}</p></div>
  <div class="actions"><a class="btn" href="{{ hk.path }}/catalogue/packages">{{ locale.hk_volver_a_los_packs|raw }}</a></div>
</div>

<div class="card">
  <div class="card-body">
    <form class="form" method="post" action="{{ hk.path }}/catalogue/packages/edit{% if cataloguePackage != null %}?id={{ cataloguePackage.id() }}{% endif %}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <div class="row r-2">
        <div class="field"><label>{{ locale.hk_codigo_de_venta_del_pack|raw }}</label><input type="text" name="salecode" class="mono" required value="{% if cataloguePackage != null %}{{ cataloguePackage.saleCode() }}{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_id_de_definicion_de_furni|raw }}</label><input type="number" name="definition_id" value="{% if cataloguePackage != null %}{{ cataloguePackage.definitionId() }}{% else %}0{% endif %}"></div>
      </div>
      <div class="row r-2">
        <div class="field"><label>{{ locale.hk_sprite_especial|raw }}</label><input type="text" name="special_sprite_id" class="mono" value="{% if cataloguePackage != null %}{{ cataloguePackage.specialSpriteId() }}{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_cantidad|raw }}</label><input type="number" name="amount" min="1" value="{% if cataloguePackage != null %}{{ cataloguePackage.amount() }}{% else %}1{% endif %}"></div>
      </div>
      <div class="form-actions"><button class="btn primary" type="submit">{% if cataloguePackage != null %}{{ locale.hk_guardar_fila|raw }}{% else %}{{ locale.hk_crear_fila|raw }}{% endif %}</button></div>
    </form>
  </div>
</div>

{% if cataloguePackage != null %}
<div class="card danger-zone mt">
  <div class="card-head"><h2>{{ locale.hk_zona_peligrosa|raw }}</h2></div>
  <div class="card-body">
    <form class="inline" method="post" action="{{ hk.path }}/catalogue/packages/delete?id={{ cataloguePackage.id() }}" data-confirm="{{ locale.hk_borrar_esta_fila_del_pack|raw }}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <button class="btn danger" type="submit">{{ locale.hk_borrar_fila|raw }}</button>
    </form>
  </div>
</div>
{% endif %}
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
