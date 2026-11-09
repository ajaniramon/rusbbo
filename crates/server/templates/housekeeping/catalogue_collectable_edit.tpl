{% set title = (collectable != null) ? (locale.hk_editar_ciclo_de_coleccionables ~ collectable.id()) : locale.hk_nuevo_ciclo_de_coleccionables %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="page-head">
  <div><p>{{ locale.hk_la_pagina_de_tienda_muestra_el_coleccionable_act|raw }}</p></div>
  <div class="actions"><a class="btn" href="{{ hk.path }}/catalogue/collectables">{{ locale.hk_volver|raw }}</a></div>
</div>

<div class="card">
  <div class="card-body">
    <form class="form" method="post" action="{{ hk.path }}/catalogue/collectables/edit{% if collectable != null %}?id={{ collectable.id() }}{% endif %}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <div class="row r-3">
        <div class="field"><label>{{ locale.hk_id_pagina_tienda|raw }}</label><input type="number" name="store_page" value="{% if collectable != null %}{{ collectable.storePageId() }}{% else %}0{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_id_pagina_admin|raw }}</label><input type="number" name="admin_page" value="{% if collectable != null %}{{ collectable.adminPageId() }}{% else %}0{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_posicion_actual|raw }}</label><input type="number" name="current_position" min="0" value="{% if collectable != null %}{{ collectable.currentPosition() }}{% else %}0{% endif %}"></div>
      </div>
      <div class="row r-2">
        <div class="field"><label>{{ locale.hk_caducidad_timestamp_unix|raw }}</label><input type="number" name="expiry" value="{% if collectable != null %}{{ collectable.expiry() }}{% else %}-1{% endif %}"><span class="help">{{ locale.hk_1_calcularla_al_arrancar|raw }}</span></div>
        <div class="field"><label>{{ locale.hk_duracion_de_cada_furni_segundos|raw }}</label><input type="number" name="lifetime" min="1" value="{% if collectable != null %}{{ collectable.lifetime() }}{% else %}2678400{% endif %}"><span class="help">{{ locale.hk_2678400_31_dias|raw }}</span></div>
      </div>
      <div class="field"><label>{{ locale.hk_clases_de_furni|raw }}</label><textarea name="class_names" rows="4" class="mono" required>{% if collectable != null %}{{ collectable.classNames() }}{% endif %}</textarea><span class="help">{{ locale.hk_sprites_separados_por_comas_en_el_orden_en_que_s|raw }}</span></div>
      <div class="form-actions"><button class="btn primary" type="submit">{% if collectable != null %}{{ locale.hk_guardar_ciclo|raw }}{% else %}{{ locale.hk_crear_ciclo|raw }}{% endif %}</button></div>
    </form>
  </div>
</div>

{% if collectable != null %}
<div class="card danger-zone mt">
  <div class="card-head"><h2>{{ locale.hk_zona_peligrosa|raw }}</h2></div>
  <div class="card-body">
    <form class="inline" method="post" action="{{ hk.path }}/catalogue/collectables/delete?id={{ collectable.id() }}" data-confirm="{{ locale.hk_borrar_este_ciclo_de_coleccionables|raw }}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <button class="btn danger" type="submit">{{ locale.hk_borrar_ciclo|raw }}</button>
    </form>
  </div>
</div>
{% endif %}
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
