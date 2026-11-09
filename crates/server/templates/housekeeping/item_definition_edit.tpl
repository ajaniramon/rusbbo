{% set title = (definition != null) ? (locale.hk_editar_furni ~ definition.sprite()) : locale.hk_nueva_definicion_de_furni %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="page-head">
  <div><p>{% if definition != null %}{{ locale.hk_definicion_2|raw }}{{ definition.id() }}. {% endif %}{{ locale.hk_al_guardar_se_recargan_las_definiciones_y_el_cat|raw }}</p></div>
  <div class="actions">
    <a class="btn" href="{{ hk.path }}/item_definitions">{{ locale.hk_volver|raw }}</a>
    {% if definition != null %}<a class="btn" href="{{ hk.path }}/catalogue/items?query={{ definition.sprite() }}">{{ locale.hk_ver_en_el_catalogo|raw }}</a>{% endif %}
  </div>
</div>

<div class="card">
  <div class="card-body">
    <form class="form" method="post" action="{{ hk.path }}/item_definitions/edit{% if definition != null %}?id={{ definition.id() }}{% endif %}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <div class="row r-3">
        <div class="field"><label>{{ locale.hk_sprite|raw }}</label><input type="text" name="sprite" class="mono" required value="{% if definition != null %}{{ definition.sprite() }}{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_nombre|raw }}</label><input type="text" name="name" value="{% if definition != null %}{{ definition.name() }}{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_descripcion|raw }}</label><input type="text" name="description" value="{% if definition != null %}{{ definition.description() }}{% endif %}"></div>
      </div>
      <div class="row">
        <div class="field"><label>{{ locale.hk_sprite_id|raw }}</label><input type="number" name="sprite_id" value="{% if definition != null %}{{ definition.spriteId() }}{% else %}0{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_largo|raw }}</label><input type="number" name="length" min="1" value="{% if definition != null %}{{ definition.length() }}{% else %}1{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_ancho|raw }}</label><input type="number" name="width" min="1" value="{% if definition != null %}{{ definition.width() }}{% else %}1{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_altura|raw }}</label><input type="text" name="top_height" value="{% if definition != null %}{{ definition.topHeight() }}{% else %}0.001{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_estado_maximo|raw }}</label><input type="text" name="max_status" value="{% if definition != null %}{{ definition.maxStatus() }}{% else %}0{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_alquiler_s|raw }}</label><input type="number" name="rental_time" value="{% if definition != null %}{{ definition.rentalTime() }}{% else %}-1{% endif %}"><span class="help">{{ locale.hk_1_permanente|raw }}</span></div>
      </div>
      <div class="row r-2">
        <div class="field">
          <label>{{ locale.hk_interactor|raw }}</label>
          <select name="interactor">
            {% for i in interactors %}<option value="{{ i }}" {% if (definition != null and definition.interactor() == i) or (definition == null and i == 'default') %}selected{% endif %}>{{ i }}</option>{% endfor %}
          </select>
        </div>
        <div class="field" style="justify-content:flex-end">
          <div class="form-actions">
            <label class="check"><input type="checkbox" name="is_tradable" {% if definition == null or definition.tradable() %}checked{% endif %}> {{ locale.hk_tradeable|raw }}</label>
            <label class="check"><input type="checkbox" name="is_recyclable" {% if definition == null or definition.recyclable() %}checked{% endif %}> {{ locale.hk_reciclable|raw }}</label>
          </div>
        </div>
      </div>
      <div class="field">
        <label>{{ locale.hk_comportamientos_separados_por_comas|raw }}</label>
        <input type="text" name="behaviour" class="mono" value="{% if definition != null %}{{ definition.behaviour() }}{% endif %}">
        <span class="help">{{ locale.hk_conocidos|raw }} {% for b in behaviours %}<code>{{ b }}</code>{% if not loop.last %}, {% endif %}{% endfor %}</span>
      </div>
      <div class="row r-3">
        <div class="field"><label>{{ locale.hk_ids_de_bebida|raw }}</label><input type="text" name="drink_ids" class="mono" value="{% if definition != null and definition.drinkIds() != null %}{{ definition.drinkIds() }}{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_rotaciones_permitidas|raw }}</label><input type="text" name="allowed_rotations" class="mono" value="{% if definition != null %}{{ definition.allowedRotations() }}{% else %}0,2,4,6{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_alturas|raw }}</label><input type="text" name="heights" class="mono" value="{% if definition != null and definition.heights() != null %}{{ definition.heights() }}{% endif %}"></div>
      </div>
      <div class="form-actions"><button class="btn primary" type="submit">{% if definition != null %}{{ locale.hk_guardar_definicion|raw }}{% else %}{{ locale.hk_crear_definicion|raw }}{% endif %}</button></div>
    </form>
  </div>
</div>

{% if definition != null %}
<div class="card danger-zone mt">
  <div class="card-head"><h2>{{ locale.hk_zona_peligrosa|raw }}</h2></div>
  <div class="card-body form-actions">
    <form class="inline" method="post" action="{{ hk.path }}/item_definitions/delete?id={{ definition.id() }}" data-confirm="{{ locale.hk_borrar_la_definicion|raw }} {{ definition.sprite() }}{{ locale.hk_los_furnis_existentes_de_este_tipo_pueden_dejar|raw }}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <button class="btn danger" type="submit">{{ locale.hk_borrar_definicion|raw }}</button>
    </form>
    <span class="help">{{ locale.hk_solo_para_definiciones_creadas_por_error_y_sin_f|raw }}</span>
  </div>
</div>
{% endif %}
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
